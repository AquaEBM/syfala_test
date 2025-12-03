//! Network implementation: all packet parsing is done here.

use super::*;

// All packets not starting with either of these are not valid as per our protocol.
// This not only allows us to identify and distinguish packets that are part of it,
// but also quickly eliminate any foreign traffic by only inspecting the first 4 bytes

// Client -> Server

// Payload: empty (for now)
const PACKET_TYPE_ID_CLIENT_DISC: [u8; 4] = *b"SyFd";
// Payload: 8 bytes (u64 timestamp) + samples (variable length (0..))
const PACKET_TYPE_ID_CLIENT_AUDIO: [u8; 4] = *b"SyFa";

// Server -> Client

// Payload: 4 bytes (channel count: u32) + 4
const PACKET_TYPE_ID_SERVER_CONFIG: [u8; 4] = *b"SyFc";

// limit packet sizes to this
// TODO: we should, nonetheless, still accept larger packets
const MAX_DATAGRAM_SIZE: num::NonZeroUsize = nz(1452);

pub mod client {
    use super::*;

    /// Sends a discovery packet to `dest_addr` address using the given `socket`
    /// Be sure to enable broadcast if `dest_addr` is a broadcast address
    #[inline]
    pub fn send_discovery(
        socket: &std::net::UdpSocket,
        dest_addr: core::net::SocketAddr,
    ) -> io::Result<()> {
        let err = socket.send_to(&PACKET_TYPE_ID_CLIENT_DISC, dest_addr);

        if err? != PACKET_TYPE_ID_CLIENT_DISC.len() {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }

    #[inline(always)]
    fn parse_config(packet: &[u8]) -> Option<AudioConfig> {
        let (_id, rem) = packet
            .split_first_chunk()
            .filter(|&(&message, _)| message == PACKET_TYPE_ID_SERVER_CONFIG)?;

        let (&n_channels, rem) = rem.split_first_chunk()?;
        let n_channels = u32::from_le_bytes(n_channels).try_into().unwrap();

        let (&buffer_size_frames, _rem) = rem.split_first_chunk()?;
        let buffer_size_frames = u32::from_le_bytes(buffer_size_frames).try_into().unwrap();

        Some(AudioConfig::new(n_channels, buffer_size_frames))
    }

    const SERVER_CONFIG_PACKET_LEN: usize =
        // packet_id: 'SyFc' (4 bytes), little endian
        size_of::<[u8 ; 4]>()
        // channel count: 4 bytes (u32), non zero, little endian
        + size_of::<u32>()
        // buffer size in frames: 4 bytes (u32), non zero, little endian
        + size_of::<u32>();

    /// Attempts to receive a server configuration from this socket. If `None` is returned,
    /// then, a packet was received that wasn't a configuration packet 
    #[inline(always)]
    pub fn try_recv_config(
        socket: &std::net::UdpSocket,
    ) -> io::Result<(core::net::SocketAddr, Option<AudioConfig>)> {
        let mut packet_buf = [0u8; SERVER_CONFIG_PACKET_LEN];

        let (bytes_read, peer_addr) = socket.recv_from(&mut packet_buf)?;

        Ok((peer_addr, parse_config(&packet_buf[..bytes_read])))
    }

    /// Allows for writing iterators of samples over the network, using our protocol
    pub struct AudioSender {
        chunk_size_spls: num::NonZeroUsize,
        // hehehe zero copy yoohoo
        scratch_buffer: arrayvec::ArrayVec<u8, { MAX_DATAGRAM_SIZE.get() }>,
    }

    impl AudioSender {
        #[inline(always)]
        pub fn new(chunk_size_spls: num::NonZeroUsize) -> Self {
            let mut scratch_buffer = arrayvec::ArrayVec::new_const();
            scratch_buffer.extend(PACKET_TYPE_ID_CLIENT_AUDIO);
            scratch_buffer.extend(0u64.to_le_bytes());

            Self {
                scratch_buffer,
                chunk_size_spls,
            }
        }

        #[inline(always)]
        fn split(&self) -> (u64, &[u8]) {
            // the buffer always contains at least 12 bytes (id + timestamp)
            let (timestamp, sample_data) = self.scratch_buffer[4..].split_at(size_of::<u64>());
            (
                u64::from_le_bytes(timestamp.try_into().unwrap()),
                sample_data,
            )
        }

        #[inline(always)]
        pub const fn chunk_size_samples(&self) -> num::NonZeroUsize {
            self.chunk_size_spls
        }

        #[inline(always)]
        pub const fn set_chunk_size_samples(&mut self, size: num::NonZeroUsize) {
            self.chunk_size_spls = size;
        }

        #[inline(always)]
        fn n_stored_samples(&self) -> usize {
            self.split().1.len() / SAMPLE_SIZE
        }

        #[inline(always)]
        pub fn current_timestamp_samples(&self) -> u64 {
            self.split()
                .0
                .strict_add(self.n_stored_samples().try_into().unwrap())
        }

        #[inline(always)]
        fn n_remaining_chunk_samples(&self) -> num::NonZeroUsize {
            let chunk_size_samples = num::NonZeroU64::try_from(self.chunk_size_samples()).unwrap();
            // Never zero, we always flush at least as soon as the buffer is full
            let max_samples_left = num::NonZeroU64::new(
                (self.scratch_buffer.remaining_capacity() / SAMPLE_SIZE)
                    .try_into()
                    .unwrap(),
            )
            .unwrap();

            // Never zero, we always flush at least at chunk boundaries
            let chunk_samples_left = num::NonZeroU64::new(
                chunk_size_samples
                    .get()
                    .strict_sub(self.current_timestamp_samples() % chunk_size_samples),
            )
            .unwrap();

            max_samples_left.min(chunk_samples_left).try_into().unwrap()
        }

        /// It is not recommended to call this function directly. using `send` provides
        /// more consistent buffer. We still provide it however, if needed.
        #[inline]
        pub fn flush(
            &mut self,
            socket: &std::net::UdpSocket,
            addr: &core::net::SocketAddr,
        ) -> io::Result<()> {
            let (timestamp, sample_data) = self.split();

            let n_samples = u64::try_from(sample_data.len() / SAMPLE_SIZE).unwrap();

            socket.send_to(self.scratch_buffer.as_slice(), addr)?;

            let _ = self.scratch_buffer.drain(4..).count();

            self.scratch_buffer
                .extend(u64::to_le_bytes(timestamp + n_samples));
            Ok(())
        }

        /// Trys Sends the provided iterator of samples using the given `socket`.
        /// 
        /// Returns `true` if data was flushed from the buffer. `false` means that not enough
        /// data was given.
        #[inline]
        pub fn send(
            &mut self,
            socket: &std::net::UdpSocket,
            addr: &core::net::SocketAddr,
            samples: impl IntoIterator<Item = Sample>,
        ) -> io::Result<bool> {
            let mut rem = self.n_remaining_chunk_samples();
            let mut used_network = false;

            for sample in samples {
                self.scratch_buffer.extend(sample.to_le_bytes());

                rem = if let Some(next) = num::NonZeroUsize::new(rem.get() - 1) {
                    next
                } else {
                    self.flush(socket, addr)?;
                    used_network = true;
                    self.n_remaining_chunk_samples()
                };
            }

            Ok(used_network)
        }
    }
}

pub mod server {
    use super::*;

    pub enum ServerMessage<T> {
        ClientDiscovery,
        ClientAudio { timestamp: u64, samples: T },
    }

    #[inline]
    pub fn recv_message<'b>(
        socket: &std::net::UdpSocket,
        buf: &'b mut [u8],
    ) -> io::Result<(
        core::net::SocketAddr,
        Option<ServerMessage<impl Iterator<Item = f32> + use<'b>>>,
    )> {
        let (bytes_read, peer_addr) = socket.recv_from(buf)?;

        let message = buf[..bytes_read]
            .split_first_chunk()
            .and_then(|(&id, payload)| {
                if id == PACKET_TYPE_ID_CLIENT_AUDIO {
                    let Some((&timestamp, sample_bytes)) = payload.split_first_chunk() else {
                        return None;
                    };

                    let timestamp = u64::from_le_bytes(timestamp);

                    let samples = sample_bytes
                        .as_chunks()
                        .0
                        .iter()
                        .copied()
                        .map(u32::from_le_bytes)
                        .map(f32::from_bits);

                    Some(ServerMessage::ClientAudio { timestamp, samples })
                } else if id == PACKET_TYPE_ID_CLIENT_DISC {
                    Some(ServerMessage::ClientDiscovery)
                } else {
                    None
                }
            });

        return Ok((peer_addr, message));
    }

    const SERVER_CONFIG_PACKET_LEN: usize =
        // Packet id (4 bytes) (little endian)
        PACKET_TYPE_ID_SERVER_CONFIG.len()
        // channel count (4 bytes, non zero) (little endian)
        + size_of::<u32>()
        // buffer size in frames (4 bytes, non zero) (little endian)
        + size_of::<u32>();

    #[inline]
    pub fn send_config(
        socket: &std::net::UdpSocket,
        dest_addr: core::net::SocketAddr,
        config: AudioConfig,
    ) -> io::Result<()> {
        let mut packet_buf = [0u8; SERVER_CONFIG_PACKET_LEN];

        let (packet_type, rem) = packet_buf.split_first_chunk_mut().unwrap();
        *packet_type = PACKET_TYPE_ID_SERVER_CONFIG;

        let (channel_count, rem) = rem.split_first_chunk_mut().unwrap();
        *channel_count = config.n_channels().get().to_le_bytes();

        let (buffer_size, rem) = rem.split_first_chunk_mut().unwrap();
        *buffer_size = config.chunk_size_frames().get().to_le_bytes();

        assert!(rem.is_empty(), "ERROR: missing fields");

        let res = socket.send_to(&packet_buf, dest_addr);

        if res? != SERVER_CONFIG_PACKET_LEN {
            Err(io::ErrorKind::Other.into())
        } else {
            Ok(())
        }
    }
}
