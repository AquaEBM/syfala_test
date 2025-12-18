use super::*;

struct AudioReceiver {
    rx: queue::Receiver,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
    zero_timestamp: cell::OnceCell<u64>,
    delay_in_samples: usize,
}

impl AudioReceiver {
    #[inline(always)]
    fn new(
        rx: queue::rtrb::Consumer<f32>,
        waker: syfala_net::Waker,
        delay_frames: usize,
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
    ) -> Option<Self> {
        interleaver::Interleaver::new(ports).map(|interleaver| Self {
            rx: queue::Receiver::with_waker(rx, waker),
            delay_in_samples: delay_frames.strict_mul(interleaver.len().get()),
            interleaver,
            zero_timestamp: cell::OnceCell::new(),
        })
    }
}

impl jack::ProcessHandler for AudioReceiver {
    #[inline(always)]
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        if self.rx.n_available_samples() == 0 {
            return jack::Control::Continue;
        }

        let jack_timestamp = u64::from(scope.last_frame_time());

        let &zero_timestamp = self.zero_timestamp.get_or_init(|| jack_timestamp);

        let timer_timestamp = jack_timestamp
            .checked_sub(zero_timestamp)
            // At the time of writing, PipeWire's JACK shim is susceptible of triggering this
            .expect("JACK ERROR: buggy frame clock")
            .strict_mul(self.interleaver.len().get().try_into().unwrap());

        let mut interleaved = self.interleaver.interleave(scope);
        let delay = self.delay_in_samples.min(interleaved.len());

        self.delay_in_samples = self.delay_in_samples.strict_sub(delay);

        // skip the first <delay> samples (jack buffers are already zeroed)
        if let Some(d) = num::NonZeroUsize::new(delay) {
            interleaved.nth(d.get().strict_sub(1));
        }

        for (jack_sample, rb_sample) in iter::zip(interleaved, self.rx.recv(timer_timestamp)) {
            *jack_sample = rb_sample;
        }

        jack::Control::Continue
    }
}

struct JackSender {
    // we store this here to keep the jack client alive
    #[allow(dead_code)]
    async_client: jack::AsyncClient<(), AudioReceiver>,
    audio_tx: queue::Sender,
}

impl TimedSender for JackSender {
    #[inline(always)]
    fn send(&mut self, timestamp: u64, samples: impl IntoIterator<Item = f32>) -> usize {
        self.audio_tx.send(timestamp, samples)
    }
}

pub fn start(
    socket: &std::net::UdpSocket,
    n_channels: num::NonZeroU32,
    mut rb_length: impl FnMut(core::net::SocketAddr, AudioConfig) -> core::time::Duration,
    mut delay: impl FnMut(core::net::SocketAddr, AudioConfig) -> core::time::Duration,
) -> io::Result<Infallible> {
    syfala_net::start_server(socket, |addr, req_config| {
        println!("Creating JACK client...");

        let name = format!("Client\n{}\n{}", addr.ip(), addr.port());
        let (jack_client, _status) =
            jack::Client::new(name.as_str(), jack::ClientOptions::NO_START_SERVER).ok()?;

        let config = req_config.unwrap_or(AudioConfig::new(
            n_channels,
            jack_client.buffer_size().try_into().unwrap(),
        ));
        let n_ports = num::NonZeroUsize::try_from(config.n_channels()).unwrap();

        let sr = jack_client.sample_rate() as u128;

        let rb_size_frames = (rb_length(addr, config).as_nanos() * sr / 1_000_000_000) as usize;

        let rb_size_spls = rb_size_frames.checked_mul(n_ports.get()).unwrap();

        println!("Allocating Ring Buffer ({rb_size_spls} samples)");

        let (tx, rx) = queue::rtrb::RingBuffer::<f32>::new(rb_size_spls);

        let audio_rx = AudioReceiver::new(
            rx,
            syfala_net::Waker::useless(),
            usize::try_from(delay(addr, config).as_nanos() * sr / 1_000_000_000).unwrap(),
            (1..=n_ports.get()).map(|i| {
                jack_client
                    .register_port(&format!("output_{i}"), jack::AudioOut::default())
                    .unwrap()
            }),
        )
        .unwrap();

        let audio_tx = queue::Sender::new(tx);

        let async_client = jack_client.activate_async((), audio_rx).ok()?;

        Some((
            JackSender {
                async_client,
                audio_tx,
            },
            config,
        ))
    })
}
