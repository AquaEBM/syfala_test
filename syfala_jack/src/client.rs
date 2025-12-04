use super::*;

use network::client;

struct AudioSender {
    tx: queue::Sender,
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
    timestamp_unset: bool,
}

impl AudioSender {
    #[inline(always)]
    fn new(
        tx: queue::rtrb::Producer<f32>,
        waker: syfala_net::Waker,
        ports: impl IntoIterator<Item = jack::Port<jack::AudioIn>>,
    ) -> Option<Self> {
        interleaver::Interleaver::new(ports).map(|interleaver| Self {
            tx: queue::Sender::with_waker(tx, waker),
            interleaver,
            timestamp_unset: true,
        })
    }
}

impl jack::ProcessHandler for AudioSender {
    #[inline]
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        let timestamp = u64::from(scope.last_frame_time());

        // set the timestamp on the first process cycle
        if mem::take(&mut self.timestamp_unset) {
            self.tx.set_zero_timestamp(timestamp);
        }

        let _spls_written = self
            .tx
            .send(timestamp, self.interleaver.interleave(scope).copied())
            .expect("ERROR: Huge drift");

        jack::Control::Continue
    }
}

struct NetworkSender {
    sender: client::AudioSender,
    rx: queue::rtrb::Consumer<f32>,
}

impl NetworkSender {
    #[inline(always)]
    fn new(rx: queue::rtrb::Consumer<f32>, chunk_size_spls: num::NonZeroUsize) -> Self {
        Self {
            sender: client::AudioSender::new(chunk_size_spls),
            rx,
        }
    }

    #[inline]
    fn try_send(
        &mut self,
        socket: &std::net::UdpSocket,
        addr: &core::net::SocketAddr,
    ) -> io::Result<bool> {
        let slots = self.rx.slots();
        let chunk = self.rx.read_chunk(slots).unwrap();

        self.sender.send(socket, addr, chunk)
    }
}

const DEFAULT_RB_SIZE_SECS: f64 = 4.;

fn start_jack_client(
    name: &str,
    config: &AudioConfig,
    network_thread_handle: thread::Thread,
) -> Result<(jack::AsyncClient<(), AudioSender>, NetworkSender), jack::Error> {
    let n_ports = num::NonZeroUsize::try_from(config.n_channels()).unwrap();
    let chunk_size_spls = num::NonZeroUsize::try_from(config.chunk_size_samples()).unwrap();

    println!("Creating JACK client...");
    let (jack_client, _status) = jack::Client::new(name, jack::ClientOptions::NO_START_SERVER)?;

    let rb_size_frames =
        num::NonZeroUsize::new((DEFAULT_RB_SIZE_SECS * jack_client.sample_rate() as f64) as usize)
            .unwrap();

    let rb_size_spls = rb_size_frames.checked_mul(n_ports).unwrap();

    println!("Allocating Ring Buffer ({rb_size_spls} samples)");

    let (tx, rx) = queue::rtrb::RingBuffer::<f32>::new(rb_size_spls.get());

    let waker = syfala_net::Waker::new(network_thread_handle, chunk_size_spls);

    let sender = AudioSender::new(
        tx,
        waker,
        (1..=n_ports.get()).map(|i| {
            jack_client
                .register_port(&format!("input_{i}"), jack::AudioIn::default())
                .unwrap()
        }),
    )
    .unwrap();

    let receiver = NetworkSender::new(rx, chunk_size_spls);

    let async_client = jack_client.activate_async((), sender)?;

    Ok((async_client, receiver))
}

#[inline(always)]
fn run_beacon(
    socket: &std::net::UdpSocket,
    dest_addr: core::net::SocketAddr,
    period: core::time::Duration,
) -> io::Result<Infallible> {
    loop {
        client::send_discovery(&socket, dest_addr)?;
        thread::sleep(period);
    }
}

#[inline(always)]
fn config_listen_run(
    socket: &std::net::UdpSocket,
    mut config_tx: queue::rtrb::Producer<(core::net::SocketAddr, AudioConfig)>,
) -> io::Result<Infallible> {
    loop {
        if let (peer_addr, Some(config)) = client::try_recv_config(socket)? {
            config_tx
                .push((peer_addr, config))
                .expect("config tx too contended!");
        }
    }
}

#[inline(always)]
fn run_client(
    socket: &std::net::UdpSocket,
    mut config_rx: queue::rtrb::Consumer<(core::net::SocketAddr, AudioConfig)>,
    mut beacon_thread: Option<thread::ScopedJoinHandle<io::Result<Infallible>>>,
    mut listener_thread: Option<thread::ScopedJoinHandle<io::Result<Infallible>>>,
) -> io::Result<Infallible> {
    let mut client_map = HashMap::new();

    let network_thread_handle = thread::current();

    loop {
        if let Some(handle) = beacon_thread.take_if(|h| h.is_finished()) {
            return handle.join().unwrap();
        }

        if let Some(handle) = listener_thread.take_if(|h| h.is_finished()) {
            return handle.join().unwrap();
        }

        // TODO? Ideally, this should be done on another thread. The current approach has the
        // advantage of requiring way less bookkeeping, but might stall audio sending a bit
        while let Ok((addr, config)) = config_rx.pop() {
            match client_map.entry(addr) {
                Entry::Occupied(e) => {
                    let (old_sender, old_config, old_jack_client) = e.into_mut();
                    if &config != old_config {
                        if let Ok((jack_client, sender)) = start_jack_client(
                            format!("SyFaLa\n{}\n{}", addr.ip(), addr.port()).as_str(),
                            &config,
                            network_thread_handle.clone(),
                        ) {
                            let _ = mem::replace(old_jack_client, jack_client);
                            let _ = mem::replace(old_sender, sender);
                            let _ = mem::replace(old_config, config);
                        }
                    }
                }
                Entry::Vacant(e) => {
                    if let Ok((jack_client, sender)) = start_jack_client(
                        format!("Syfala\n{}\n{}", addr.ip(), addr.port()).as_str(),
                        &config,
                        network_thread_handle.clone(),
                    ) {
                        e.insert((sender, config, jack_client));
                    }
                }
            }
        }

        let mut any_ready = false;

        for (addr, (sender, _, _)) in client_map.iter_mut() {
            any_ready |= sender.try_send(socket, addr)?;
        }

        if !any_ready {
            std::thread::park_timeout(core::time::Duration::from_millis(75));
        }
    }
}

const EVENT_QUEUE_CAPACITY: usize = 1024;

#[inline]
pub fn start(
    socket: &std::net::UdpSocket,
    beacon_dest: core::net::SocketAddr,
    beacon_period: core::time::Duration,
) -> io::Result<Infallible> {
    let (config_tx, config_rx) = queue::rtrb::RingBuffer::new(EVENT_QUEUE_CAPACITY);

    thread::scope(|s| {
        // Thread 1: beacon
        let beacon_thread = s.spawn(|| run_beacon(&socket, beacon_dest, beacon_period));

        // Thread 2: listen for and report responses
        let listener_thread = s.spawn(|| config_listen_run(&socket, config_tx));

        // Thread 3: audio sending and jack client creation
        run_client(
            &socket,
            config_rx,
            Some(beacon_thread),
            Some(listener_thread),
        )
    })
}
