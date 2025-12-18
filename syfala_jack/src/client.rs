use super::*;

struct AudioSender {
    tx: queue::Sender,
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
    zero_timestamp: cell::OnceCell<u64>,
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
            zero_timestamp: cell::OnceCell::new(),
        })
    }
}

impl jack::ProcessHandler for AudioSender {
    #[inline]
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        let jack_timestamp = u64::from(scope.last_frame_time());
        // Set the first timestamp on the first process cycle
        let &zero_timestamp = self.zero_timestamp.get_or_init(|| jack_timestamp);

        let timer_timestamp = jack_timestamp
            .checked_sub(zero_timestamp)
            // At the time of writing, PipeWire's JACK shim is susceptible of triggering this
            .expect("JACK ERROR: buggy frame clock")
            .strict_mul(self.interleaver.len().get().try_into().unwrap());

        let samples = self.interleaver.interleave(scope).copied();

        self.tx.send(timer_timestamp, samples); 

        jack::Control::Continue
    }
}

struct JackReceiver {
    // we store this here to keep the jack client alive
    #[allow(dead_code)]
    async_client: jack::AsyncClient<(), AudioSender>,
    audio_rx: queue::Receiver,
}

impl TimedReceiver for JackReceiver {
    fn recv(&mut self, sample_idx: u64) -> impl Iterator<Item = f32> {
        self.audio_rx.recv(sample_idx)
    }
}

pub fn start(
    socket: &std::net::UdpSocket,
    beacon_dest: core::net::SocketAddr,
    beacon_period: core::time::Duration,
    mut rb_length: impl FnMut(core::net::SocketAddr, AudioConfig) -> core::time::Duration,
    mut delay: impl FnMut(core::net::SocketAddr, AudioConfig) -> core::time::Duration,
) -> io::Result<Infallible> {
    syfala_net::start_client(
        socket,
        beacon_dest,
        beacon_period,
        None,
        |addr, config, handle| {
            let n_ports = num::NonZeroUsize::try_from(config.n_channels()).unwrap();
            let chunk_size_spls = num::NonZeroUsize::try_from(config.chunk_size_samples()).unwrap();

            println!("Creating JACK client...");
            let name = format!("Server\n{}\n{}", addr.ip(), addr.port());

            let (jack_client, _status) =
                jack::Client::new(name.as_str(), jack::ClientOptions::NO_START_SERVER).ok()?;

            let sr = jack_client.sample_rate() as u128;

            let rb_size_frames = ((rb_length(addr, config).as_nanos() * sr) / 1_000_000_000) as usize;

            let rb_size_spls = rb_size_frames.checked_mul(n_ports.get()).unwrap();
            
            let delay_frames = usize::try_from((delay(addr, config).as_nanos() * sr) / 1_000_000_000).unwrap();
            let delay_samples = delay_frames.strict_mul(n_ports.get());

            println!("Allocating Ring Buffer ({rb_size_spls} samples)");
            println!("Adding delay ({delay_samples} samples)");

            let (mut tx, rx) =
                queue::rtrb::RingBuffer::<f32>::new(rb_size_spls.strict_add(delay_samples));

            tx.write_chunk_uninit(delay_samples)
                .unwrap()
                .fill_from_iter(iter::repeat(0.));

            let waker = syfala_net::Waker::new(handle.clone(), chunk_size_spls);

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

            let async_client = jack_client.activate_async((), sender).ok()?;
            let audio_rx = queue::Receiver::new(rx);

            Some(JackReceiver {
                async_client,
                audio_rx,
            })
        },
    )
}
