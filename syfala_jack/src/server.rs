use super::*;

use network::server;

struct AudioReceiver {
    rx: queue::Receiver,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
    timestamp_unset: bool,
}

impl AudioReceiver {
    #[inline(always)]
    fn new(
        rx: queue::rtrb::Consumer<f32>,
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

        let interleaved = self.interleaver.interleave(scope);

        let _samples = self
            .rx
            .recv(timestamp, interleaved)
            .expect("ERROR: Huge drift");
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

    let (tx, rx) = queue::rtrb::RingBuffer::<f32>::new(rb_size_spls.get());

    let audio_rx = AudioReceiver::new(
        rx,
        syfala_net::Waker::useless(),
        (1..=n_ports.get()).map(|i| {
            jack_client
                .register_port(&format!("output_{i}"), jack::AudioOut::default())
                .unwrap()
        }),
    )
    .unwrap();

    let audio_tx = queue::Sender::new(tx);

    let async_client = jack_client.activate_async((), audio_rx)?;

    Ok((async_client, audio_tx))
}

pub fn start(socket: &std::net::UdpSocket, config: AudioConfig) -> io::Result<Infallible> {
    let mut buf = [0u8; 2000];

    let mut client_map = HashMap::new();

    loop {
        let (addr, message) = server::recv_message(socket, &mut buf)?;

        if let Some(message) = message {
            match message {
                server::ServerMessage::ClientDiscovery => {
                    server::send_config(socket, addr, config)?;
                    // NIGHTLY: #[feature(map_try_insert)] use try_insert instead

                    if !client_map.contains_key(&addr) {
                        if let Ok((jack_client, sender)) = start_jack_client(
                            format!("Syfala\n{}\n{}", addr.ip(), addr.port()).as_str(),
                            &config,
                        ) {
                            client_map.insert(addr, (sender, jack_client));
                        }
                    }
                }
                server::ServerMessage::ClientAudio { timestamp, samples } => {
                    if let Some((sender, _)) = client_map.get_mut(&addr) {
                        sender.send(timestamp, samples).expect("ERROR: huge drift");
                    }
                }
            }
        }
    }
}
