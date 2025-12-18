//! Implementation of a simple protocol for real-time audio communication and discovery

use core::{convert::Infallible, num};
use std::{
    collections::{HashMap, hash_map::Entry},
    io, thread,
};

use crate::wire::CONN_PACKET_MAX_LEN;

pub mod wire;

pub mod queue;
mod timing;

#[inline(always)]
const fn nz(x: usize) -> num::NonZeroUsize {
    num::NonZeroUsize::new(x).unwrap()
}

pub const SAMPLE_SIZE: num::NonZeroUsize = nz(size_of::<f32>());

/// Represents a server's audio configuration. May have more fields in the future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioConfig {
    n_channels: num::NonZeroU32,
    buffer_size_frames: num::NonZeroU32,
}

impl AudioConfig {
    pub const fn new(n_channels: num::NonZeroU32, buffer_size_frames: num::NonZeroU32) -> Self {
        Self {
            n_channels,
            buffer_size_frames,
        }
    }

    #[inline(always)]
    pub const fn n_channels(&self) -> num::NonZeroU32 {
        self.n_channels
    }

    #[inline(always)]
    pub const fn chunk_size_frames(&self) -> num::NonZeroU32 {
        self.buffer_size_frames
    }

    /// This is the same as [`self.n_channels()`](Self::n_channels)` *
    /// `[`self.chunk_size_frames()`](Self::chunk_size_frames)
    #[inline(always)]
    pub fn chunk_size_samples(&self) -> num::NonZeroU32 {
        self.chunk_size_frames()
            .checked_mul(self.n_channels())
            .unwrap()
    }
}

/// Enables waking a thread in a periodic manner, usually used in conjunction
/// with the queues in [`queue`]. Use [`Waker::useless`] for a waker that does nothing.
#[derive(Debug, Clone)]
pub struct Waker {
    thread_handle: std::thread::Thread,
    chunk_size_spls: num::NonZeroUsize,
}

impl Default for Waker {
    fn default() -> Self {
        Self::useless()
    }
}

impl Waker {
    #[inline(always)]
    pub fn useless() -> Self {
        Self::new(std::thread::current(), num::NonZeroUsize::MAX)
    }

    #[inline(always)]
    pub const fn new(
        thread_handle: std::thread::Thread,
        chunk_size_spls: num::NonZeroUsize,
    ) -> Self {
        Self {
            thread_handle,
            chunk_size_spls,
        }
    }

    #[inline(always)]
    pub const fn chunk_size_samples(&self) -> num::NonZeroUsize {
        self.chunk_size_spls
    }

    #[inline(always)]
    pub const fn set_chunk_size_samples(&mut self, chunk_size_spls: num::NonZeroUsize) {
        self.chunk_size_spls = chunk_size_spls;
    }

    #[inline(always)]
    fn wake(&self) {
        self.thread_handle.unpark();
    }
}

// Open to bikeshedding those names

pub trait TimedSender {
    fn send(&mut self, sample_idx: u64, samples: impl IntoIterator<Item = f32>) -> usize;
}

impl<T: TimedSender + ?Sized> TimedSender for Box<T> {
    fn send(&mut self, sample_idx: u64, samples: impl IntoIterator<Item = f32>) -> usize {
        self.as_mut().send(sample_idx, samples)
    }
}

pub trait TimedReceiver {
    fn recv(&mut self, sample_idx: u64) -> impl Iterator<Item = f32>;
}

impl<T: TimedReceiver + ?Sized> TimedReceiver for Box<T> {
    fn recv(&mut self, sample_idx: u64) -> impl Iterator<Item = f32> {
        self.as_mut().recv(sample_idx)
    }
}

#[inline(always)]
const fn f32_from_bytes(&bytes: &[u8; 4]) -> f32 {
    f32::from_bits(u32::from_le_bytes(bytes))
}

const CLIENT_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(1);

pub fn start_server<T: TimedSender>(
    socket: &std::net::UdpSocket,
    mut on_connect: impl FnMut(core::net::SocketAddr, Option<AudioConfig>) -> Option<(T, AudioConfig)>,
) -> io::Result<Infallible> {
    let mut buf = [0u8; 2000];

    let mut client_map = HashMap::<_, (std::time::Instant, _)>::new();

    loop {
        
        client_map.retain(|_, (last_seen, _)| last_seen.elapsed() < CLIENT_TIMEOUT);

        let (addr, message) = match wire::Message::try_recv(socket, &mut buf) {
            Ok(ret) => ret,
            #[rustfmt::skip]
            Err(e) => if [
                io::ErrorKind::WouldBlock,
                io::ErrorKind::TimedOut,
            ].contains(&e.kind()) {
                continue;
            } else {
                return io::Result::Err(e);
            },
        };

        if let Some(message) = message {
            match message {
                wire::Message::RequestConnection(maybe_config) => {
                    // NIGHTLY: #[feature(map_try_insert)] use try_insert instead

                    if let Some((last_seen, _)) = client_map.get_mut(&addr) {
                        *last_seen = std::time::Instant::now();
                        wire::send_connection(socket, addr, None)?;
                    } else if let Some((tx, config)) = on_connect(addr, maybe_config) {
                        // TODO: we should probably offload on_connect to another thread
                        wire::send_connection(socket, addr, Some(config))?;
                        client_map.insert(addr, (std::time::Instant::now(), tx));
                    }
                }
                wire::Message::Audio {
                    timestamp,
                    sample_bytes: samples,
                } => {
                    if let Some((_, rx)) = client_map.get_mut(&addr) {
                        rx.send(timestamp, samples.as_chunks().0.iter().map(f32_from_bytes));
                    }
                }
            }
        }
    }
}

const EVENT_QUEUE_CAPACITY: usize = 1024;
const SERVER_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(1);

struct ServerEntry<T> {
    pub config: AudioConfig,
    receiver: T,
    pub timestamp: std::time::Instant,
    pub sample_idx: u64,
    network_sender: wire::AudioSender,
}

impl<T: TimedReceiver> ServerEntry<T> {
    #[inline(always)]
    fn carry_over(
        &mut self,
        sample_idx: u64,
        socket: &std::net::UdpSocket,
        addr: &core::net::SocketAddr,
    ) -> io::Result<bool> {
        self.network_sender.send(
            socket,
            &addr,
            num::NonZeroUsize::try_from(self.config.chunk_size_samples()).unwrap(),
            self.receiver
                .recv(sample_idx)
                .inspect(|_| self.sample_idx = self.sample_idx.strict_add(1)),
        )
    }
}

impl<T> ServerEntry<T> {
    #[inline(always)]
    fn new(receiver: T, config: AudioConfig) -> Self {
        Self {
            config,
            receiver,
            network_sender: wire::AudioSender::new(),
            timestamp: std::time::Instant::now(),
            sample_idx: 0,
        }
    }
}

#[inline]
pub fn start_client<T: TimedReceiver>(
    socket: &std::net::UdpSocket,
    beacon_dest: core::net::SocketAddr,
    beacon_period: core::time::Duration,
    suggest_config: Option<AudioConfig>,
    mut on_connect: impl FnMut(core::net::SocketAddr, AudioConfig, &thread::Thread) -> Option<T>,
) -> io::Result<Infallible> {
    let (mut config_tx, mut config_rx) = rtrb::RingBuffer::new(EVENT_QUEUE_CAPACITY);

    thread::scope(|s| {
        // Thread 1: beacon
        let beacon_thread = s.spawn(|| {
            loop {
                wire::send_connection(&socket, beacon_dest, suggest_config)?;
                thread::sleep(beacon_period);
            }
        });

        // Thread 2: listen for and report responses
        let listener_thread = s.spawn(|| {
            let mut buf = [0u8 ; CONN_PACKET_MAX_LEN];

            loop {
                let (peer_addr, maybe_config) = wire::Message::try_recv(socket, &mut buf).expect("no");

                if let Some(wire::Message::RequestConnection(config)) = maybe_config {
                    
                    config_tx
                        .push((peer_addr, config))
                        .expect("Error: event queue too contended")
                }
            }
        });

        let mut server_map = HashMap::new();

        let network_thread_handle = thread::current();

        loop {
            if beacon_thread.is_finished() {
                return beacon_thread.join().unwrap();
            }
            
            if listener_thread.is_finished() {
                return dbg!(listener_thread.join().unwrap());
            }

            // TODO? Ideally, this should be done on another thread. The current approach has the
            // advantage of requiring way less bookkeeping, but might stall audio sending a bit
            // depending on how slow the on_connect function is
            while let Ok((addr, config)) = config_rx.pop() {
                match server_map.entry(addr) {
                    Entry::Occupied(mut e) => {
                        let old: &mut ServerEntry<_> = e.get_mut();
                        if let Some(config) = config && config != old.config {
                            if let Some(recv) = on_connect(addr, config, &network_thread_handle) {
                                *old = ServerEntry::new(recv, config);
                            } else {
                                e.remove_entry();
                            }
                        } else {
                            old.timestamp = std::time::Instant::now();
                        }
                    }
                    Entry::Vacant(e) => {
                        if let Some(config) = config {
                            if let Some(recv) = on_connect(addr, config, &network_thread_handle) {
                                e.insert(ServerEntry::new(recv, config));
                            }
                        }
                    }
                }
            }

            server_map.retain(|_, e| e.timestamp.elapsed() < SERVER_TIMEOUT);

            let mut any_ready = false;

            for (addr, server) in server_map.iter_mut() {
                any_ready |= match server.carry_over(server.sample_idx, socket, addr) {
                    Ok(b) => b,
                    Err(e) => if [
                        io::ErrorKind::WouldBlock,
                        io::ErrorKind::TimedOut,
                    ].contains(&e.kind()) {
                        continue;
                    } else {
                        return io::Result::Err(e);
                    },
                }
            }

            if !any_ready {
                std::thread::park_timeout(core::time::Duration::from_millis(250));
            }
        }
    })
}
