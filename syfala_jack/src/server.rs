use syfala_net::queue;

use super::*;

struct AudioReceiver {
    rx: queue::Receiver,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
    timestamp_unset: bool,
}

impl AudioReceiver {
    #[inline(always)]
    fn new(
        rx: rtrb::Consumer<f32>,
        waker: syfala_net::Waker,
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
    ) -> Option<Self> {
        interleaver::Interleaver::new(ports).map(|interleaver| Self {
            rx: queue::Receiver::with_waker(rx, waker),
            interleaver,
            timestamp_unset: true,
        })
    }
}

impl jack::ProcessHandler for AudioReceiver {
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        let timestamp = u64::from(scope.last_frame_time());

        if mem::take(&mut self.timestamp_unset) {
            self.rx.set_zero_timestamp(timestamp);
        }

        let samples = self
            .rx
            .recv(timestamp, scope.n_frames().try_into().unwrap())
            .expect("ERROR: Huge drift");

        for (in_sample, out_sample) in samples.zip(self.interleaver.interleave(scope)) {
            *out_sample = in_sample;
        }

        jack::Control::Continue
    }
}

const DEFAULT_RB_SIZE_SECS: f64 = 4.;

fn start_jack_client(
    name: &str,
    config: &AudioConfig,
) -> Result<(jack::AsyncClient<(), AudioReceiver>, queue::Sender), jack::Error> {
    let n_ports = num::NonZeroUsize::try_from(config.n_channels()).unwrap();

    println!("Creating JACK client...");
    let (jack_client, _status) = jack::Client::new(name, jack::ClientOptions::NO_START_SERVER)?;

    let rb_size_frames =
        num::NonZeroUsize::new((DEFAULT_RB_SIZE_SECS * jack_client.sample_rate() as f64) as usize)
            .unwrap();

    let rb_size_spls = rb_size_frames.checked_mul(n_ports).unwrap();

    println!("Allocating Ring Buffer ({rb_size_spls} samples)");

    let (tx, rx) = rtrb::RingBuffer::<f32>::new(rb_size_spls.get());

    let sender = AudioReceiver::new(
        rx,
        syfala_net::Waker::useless(),
        (1..=n_ports.get()).map(|i| {
            jack_client
                .register_port(&format!("output_{i}"), jack::AudioOut::default())
                .unwrap()
        }),
    )
    .unwrap();

    let receiver = queue::Sender::new(tx);

    let async_client = jack_client.activate_async((), sender)?;

    Ok((async_client, receiver))
}

struct JackClientMap {
    map: HashMap<core::net::SocketAddrV4, jack::AsyncClient<(), AudioReceiver>>,
    event_tx: rtrb::Producer<(core::net::SocketAddrV4, queue::Sender)>,
}

impl JackClientMap {
    #[inline(always)]
    pub fn new(event_tx: rtrb::Producer<(core::net::SocketAddrV4, queue::Sender)>) -> Self {
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
    ) {
        match self.map.entry(addr) {
            Entry::Occupied(_) => {}
            Entry::Vacant(e) => {
                if let Ok((jack_client, sender)) = start_jack_client(name, &config) {
                    e.insert(jack_client);
                    self.event_tx
                        .push((addr, sender))
                        .expect("ERROR: Event queue too contended!");
                }
            }
        }
    }
}

fn control_thread_run(
    config: AudioConfig,
    event_tx: rtrb::Producer<(core::net::SocketAddrV4, queue::Sender)>,
    discovery_socket_addr: core::net::SocketAddr,
    audio_socket_addr: core::net::SocketAddrV4,
) -> io::Result<Infallible> {
    let discovery_socket = std::net::UdpSocket::bind(discovery_socket_addr)?;
    let mut client_map = JackClientMap::new(event_tx);

    loop {
        if let (source, Some(addr)) = network::discovery::accept_discovery(&discovery_socket)? {
            let name = format!("SyFaLa\n{}\n{}", addr.ip(), addr.port());
            client_map.try_register_client(name.as_str(), addr, config);

            network::discovery::send_config(&discovery_socket, source, audio_socket_addr, config)?;
        }
    }
}

fn audio_network_thread_run(
    mut event_rx: rtrb::Consumer<(core::net::SocketAddrV4, queue::Sender)>,
    audio_socket_addr: core::net::SocketAddrV4,
    mut control_thread_handle: Option<thread::JoinHandle<io::Result<Infallible>>>,
) -> io::Result<Infallible> {
    let mut tx_map = HashMap::new();
    let audio_socket = std::net::UdpSocket::bind(audio_socket_addr)?;

    // The main network thread loop
    loop {
        while let Ok((addr, tx)) = event_rx.pop() {
            tx_map.insert(addr, tx);
        }

        let (source, timestamp, samples) = network::recv_audio_packet(&audio_socket)?;

        let core::net::SocketAddr::V4(source) = source else {
            continue;
        };

        if let Some(tx) = tx_map.get_mut(&source) {
            tx.send(timestamp, samples).expect("ERROR: drift too huge");
        }

        if let Some(handle) = control_thread_handle.take_if(|h| h.is_finished()) {
            return handle.join().unwrap();
        }
    }
}

const DEFAULT_DISCOVERY_SOCKET_ADDR: core::net::SocketAddrV4 =
    core::net::SocketAddrV4::new(core::net::Ipv4Addr::LOCALHOST, 4451);

const DEFAULT_AUDIO_SOCKET_ADDR: core::net::SocketAddrV4 =
    core::net::SocketAddrV4::new(core::net::Ipv4Addr::LOCALHOST, 6910);

const EVENT_QUEUE_LEN: num::NonZeroUsize = num::NonZeroUsize::new(1024).unwrap();

pub fn jack_server_run() -> io::Result<Infallible> {
    let (event_tx, event_rx) = rtrb::RingBuffer::new(EVENT_QUEUE_LEN.get());

    let control_thread_handle = thread::spawn(move || {
        control_thread_run(
            AudioConfig::new(
                num::NonZeroU32::new(8).unwrap(),
                num::NonZeroU32::new(16).unwrap(),
            ),
            event_tx,
            DEFAULT_DISCOVERY_SOCKET_ADDR.into(),
            DEFAULT_AUDIO_SOCKET_ADDR,
        )
    });

    audio_network_thread_run(
        event_rx,
        DEFAULT_AUDIO_SOCKET_ADDR,
        Some(control_thread_handle),
    )
}
