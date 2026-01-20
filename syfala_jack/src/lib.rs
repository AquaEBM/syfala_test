use core::{cell::OnceCell, num};

mod interleaver;

// We might have to do some manual encoding/decoding here, sadly.
// Let's hope postcard and our utilities crate can help...

const SAMPLE_SIZE: num::NonZeroUsize = num::NonZeroUsize::new(size_of::<f32>()).unwrap();

/// Wraps a [`syfala_utils::MultichannelTx`] alongside an [`interleaver::Interleaver<AudioIn>`]
/// 
/// Used in [`DuplexProcessHandler`]
pub struct JackMultichannelTx {
    tx: syfala_utils::queue::rtrb::Producer<u8>,
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
}

impl JackMultichannelTx {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioIn>>,
        tx: syfala_utils::queue::rtrb::Producer<u8>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { tx, interleaver })
    }
}

/// Wraps a [`syfala_utils::MultichannelRx`] alongside an [`interleaver::Interleaver<AudioOut>`]
///
/// Used in [`DuplexProcessHandler`].
pub struct JackMultichannelRx {
    rx: syfala_utils::queue::rtrb::Consumer<u8>,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
}

impl JackMultichannelRx {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
        rx: syfala_utils::queue::rtrb::Consumer<u8>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { rx, interleaver })
    }
}

// TODO: replace the old ProcessSender and ProcessReceiver with this big boi

pub struct DuplexProcessHandler {
    txs: Box<[JackMultichannelTx]>,
    rxs: Box<[JackMultichannelRx]>,
    current_frame_idx: OnceCell<u64>,
}

impl jack::ProcessHandler for DuplexProcessHandler {
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        let frame_idx = u64::from(scope.last_frame_time());
        let n_frames = scope.n_frames();

        for JackMultichannelTx { tx, interleaver } in &mut self.txs {
            let n_channels = interleaver.len();
            let frame_size_bytes = SAMPLE_SIZE.checked_mul(n_channels).unwrap();
            let n_bytes_total = frame_size_bytes.get().checked_mul(n_frames.try_into().unwrap());

            let mut chunk = syfala_utils::queue::producer_get_all(tx);

            let writer = syfala_utils::queue::chunk_get_writer(&mut chunk);

            
        }

        // TODO...

        jack::Control::Continue
    }
}
