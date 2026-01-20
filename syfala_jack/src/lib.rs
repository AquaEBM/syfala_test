use core::{cell::OnceCell, num};

mod interleaver;

// We might have to do some manual encoding/decoding here, sadly.
// Let's hope postcard and our utilities crate can help...


const SAMPLE_SIZE: num::NonZeroUsize = num::NonZeroUsize::new(size_of::<f32>()).unwrap();

/// Wraps a [`syfala_utils::MultichannelTx`] alongside an [`interleaver::Interleaver<AudioIn>`]
/// 
/// Used in [`DuplexProcessHandler`]
pub struct JackMultichannelTx {
    tx: syfala_utils::MultichannelTx,
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
}

impl JackMultichannelTx {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioIn>>,
        tx: syfala_utils::queue::rtrb::Producer<u8>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self {
            tx: syfala_utils::MultichannelTx::new(tx, interleaver.len()),
            interleaver,
        })
    }
}


/// Wraps a [`syfala_utils::MultichannelRx`] alongside an [`interleaver::Interleaver<AudioOut>`]
/// 
/// Used in [`DuplexProcessHandler`].
pub struct JackMultichannelRx {
    rx: syfala_utils::MultichannelRx,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
}

impl JackMultichannelRx {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
        rx: syfala_utils::queue::rtrb::Consumer<u8>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self {
            rx: syfala_utils::MultichannelRx::new(rx, interleaver.len()),
            interleaver,
        })
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
        let idx = u64::from(scope.last_frame_time());
        let &prev_frame_idx = self.current_frame_idx.get_or_init(|| idx);
        let n_frames_skipped = idx
            .checked_sub(prev_frame_idx)
            // At the time of writing, PipeWire's JACK shim is susceptible of triggering this
            // :(
            .expect("JACK ERROR: buggy frame counter");

        // TODO...

        jack::Control::Continue
    }
}