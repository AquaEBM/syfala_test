use super::*;

struct AudioSender {
    tx: queue::Sender,
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
    timestamp_unset: bool,
}

impl AudioSender {
    #[inline(always)]
    fn new(
        tx: rtrb::Producer<f32>,
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
    sender: network::Sender,
    rx: rtrb::Consumer<f32>,
}

impl NetworkSender {
    #[inline(always)]
    fn new(rx: rtrb::Consumer<f32>, chunk_size_spls: num::NonZeroUsize) -> Self {
        Self {
            sender: network::Sender::new(chunk_size_spls),
            rx,
        }
    }

    #[inline]
    fn try_send(
        &mut self,
        socket: &std::net::UdpSocket,
        addr: core::net::SocketAddr,
    ) -> io::Result<bool> {
        self.sender.send(
            socket,
            addr,
            self.rx.read_chunk(self.rx.slots()).unwrap().into_iter(),
        )
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

    let (tx, rx) = rtrb::RingBuffer::<f32>::new(rb_size_spls.get());

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

struct JackClientMap {
    map: HashMap<core::net::SocketAddrV4, (AudioConfig, jack::AsyncClient<(), AudioSender>)>,
    event_tx: rtrb::Producer<(core::net::SocketAddrV4, NetworkSender)>,
}

impl JackClientMap {
    #[inline(always)]
    pub fn new(event_tx: rtrb::Producer<(core::net::SocketAddrV4, NetworkSender)>) -> Self {
        Self {
            map: HashMap::new(),
            event_tx,
        }
    }

    #[inline]
    pub fn try_register_client(
        &mut self,
        name: &str,
        addr: core::net::SocketAddrV4,
        config: AudioConfig,
        network_thread_handle: &thread::Thread,
    ) {
        match self.map.entry(addr) {
            Entry::Occupied(mut e) => {
                let (old_config, _) = e.get();

                if old_config != &config {
                    if let Ok((jack_client, net_sender)) =
                        start_jack_client(name, &config, network_thread_handle.clone())
                    {
                        // the old client gets deactivated automatically here, in it's destructor
                        let (_old_config, _old_client) = e.insert((config, jack_client));
                        self.event_tx
                            .push((addr, net_sender))
                            .expect("ERROR: event queue too contended!");
                    }
                }
            }
            Entry::Vacant(e) => {
                if let Ok((jack_client, net_sender)) =
                    start_jack_client(name, &config, network_thread_handle.clone())
                {
                    e.insert((config, jack_client));
                    self.event_tx
                        .push((addr, net_sender))
                        .expect("ERROR: event queue too contended!");
                }
            }
        }
    }
}

fn control_thread_run(
    event_tx: rtrb::Producer<(core::net::SocketAddrV4, NetworkSender)>,
    network_thread_handle: thread::Thread,
    discovery_socket_addr: core::net::SocketAddr,
    beacon_dest_addr: core::net::SocketAddr,
    audio_socket_addr: core::net::SocketAddrV4,
    beacon_period: core::time::Duration,
) -> io::Result<Infallible> {
    let discovery_socket = std::net::UdpSocket::bind(discovery_socket_addr)?;
    discovery_socket.set_read_timeout(Some(core::time::Duration::from_millis(500)))?;
    let mut client_map = JackClientMap::new(event_tx);

    thread::scope(|s| {
        // Thread 1: beacon
        let beacon_thread_handle = s.spawn(|| {
            loop {
                syfala_net::network::discovery::send_discovery(
                    &discovery_socket,
                    beacon_dest_addr.into(),
                    audio_socket_addr,
                )?;

                thread::sleep(beacon_period);
            }
        });

        // Thread 2: discovery
        loop {
            if beacon_thread_handle.is_finished() {
                return beacon_thread_handle.join().unwrap();
            }

            match syfala_net::network::discovery::accept_config(&discovery_socket) {
                Ok(parsed) => {
                    if let Some((addr, config)) = parsed {
                        let client_name = format!("SyFaLa\n{}\n{}", addr.ip(), addr.port());

                        // Audio (JACK) threads are created here
                        client_map.try_register_client(
                            client_name.as_str(),
                            addr,
                            config,
                            &network_thread_handle,
                        );
                    }
                },
                Err(e) => if let io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut = e.kind() {
                    continue;
                },
            }

            
        }
    })
}

fn audio_network_thread_run(
    mut event_rx: rtrb::Consumer<(core::net::SocketAddrV4, NetworkSender)>,
    audio_socket_addr: core::net::SocketAddrV4,
    mut control_thread_handle: Option<thread::JoinHandle<io::Result<Infallible>>>,
) -> io::Result<Infallible> {
    let mut rx_map = HashMap::new();
    let audio_socket = std::net::UdpSocket::bind(audio_socket_addr)?;

    // The main network thread loop
    loop {
        if let Some(handle) = control_thread_handle.take_if(|h| h.is_finished()) {
            return handle.join().unwrap();
        }

        while let Ok((addr, rx)) = event_rx.pop() {
            // insert new clients (potentially replace old ones)
            rx_map.insert(addr, rx);
        }

        let mut any_ready = false;

        for (&addr, rx) in &mut rx_map {
            any_ready |= rx.try_send(&audio_socket, addr.into())?;
        }

        if !any_ready {
            thread::park();
        }
    }
}

const DEFAULT_DISCOVERY_SENDER: core::net::SocketAddrV4 =
    core::net::SocketAddrV4::new(core::net::Ipv4Addr::LOCALHOST, 4451);

const DEFAULT_BEACON_DEST: core::net::SocketAddrV4 =
    core::net::SocketAddrV4::new(core::net::Ipv4Addr::BROADCAST, 3581);

const DEFAULT_AUDIO_SENDER: core::net::SocketAddrV4 =
    core::net::SocketAddrV4::new(core::net::Ipv4Addr::LOCALHOST, 6910);

const DEFAULT_BEACON_PERIOD: core::time::Duration = core::time::Duration::from_millis(250);

const EVENT_QUEUE_LEN: num::NonZeroUsize = num::NonZeroUsize::new(1024).unwrap();

// NIGHTLY: use !
pub fn jack_client_run() -> io::Result<Infallible> {
    let network_thread_handle = thread::current();

    let (event_tx, event_rx) =
        rtrb::RingBuffer::<(core::net::SocketAddrV4, NetworkSender)>::new(EVENT_QUEUE_LEN.get());

    let control_threads_handle = thread::spawn(move || {
        control_thread_run(
            event_tx,
            network_thread_handle,
            DEFAULT_DISCOVERY_SENDER.into(),
            DEFAULT_BEACON_DEST.into(),
            DEFAULT_AUDIO_SENDER,
            DEFAULT_BEACON_PERIOD,
        )
    });

    audio_network_thread_run(event_rx, DEFAULT_AUDIO_SENDER, Some(control_threads_handle))
}
