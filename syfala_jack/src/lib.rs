use core::cell;

pub use syfala_network as network;
pub use syfala_utils as utils;

mod interleaver;

/// The only audio sample format supported by JACK.
///
/// JACK operates exclusively on 32-bit floating point samples,
/// which matches the network protocol configuration used here.
pub const JACK_SAMPLE_TYPE: network::proto::format::SampleType =
    network::proto::format::SampleType::IEEF32;

/// Type alias for JACK audio samples.
pub type JackSample = f32;

/// Sender side of a JACK stream.
///
/// This reads audio samples from one or more JACK input ports,
/// interleaves them into a single sample stream, and forwards the
/// samples into an indexed transmit queue.
///
/// The index counter ensures that samples are written at the correct
/// logical position, even if cycles are skipped.
pub struct JackTx<C> {
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
    tx: utils::queue::IndexedTx<C, JackSample>,
}

impl<C> JackTx<C> {
    /// Creates a new transmit path from a set of JACK input ports.
    ///
    /// Returns `None` if the iterator is empty
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioIn>>,
        tx: utils::queue::IndexedTx<C, JackSample>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { interleaver, tx })
    }
}

/// Receive side of a JACK stream.
///
/// This pulls samples from an indexed receive queue and writes them
/// into JACK output ports. Incoming samples are deinterleaved so that
/// each port receives its corresponding channel.
///
/// Padding is applied automatically when samples are missing.
pub struct JackRx<C> {
    rx: utils::queue::IndexedRx<C, JackSample>,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
}

impl<C> JackRx<C> {
    /// Creates a new receive path from a set of JACK output ports.
    ///
    /// Returns `None` if the iterator is empty.
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
        rx: utils::queue::IndexedRx<C, JackSample>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { rx, interleaver })
    }
}

/// A JACK process handler supporting simultaneous input and output.
///
/// This handler manages multiple transmit and receive paths and keeps
/// them synchronized using the frame-based indices provided by JACK, during
/// process cycles.
pub struct DuplexProcessHandler<TxCounter, RxCounter> {
    txs: Box<[JackTx<TxCounter>]>,
    rxs: Box<[JackRx<RxCounter>]>,
    /// The fixed reference frame index captured on the first process call
    /// and used to compute stable sample indices for all subsequent cycles.
    start_frame_idx: cell::OnceCell<u64>,
}

impl<TxCounter, RxCounter> DuplexProcessHandler<TxCounter, RxCounter> {
    /// Creates a new duplex process handler.
    #[inline(always)]
    pub fn new(
        inputs: impl IntoIterator<Item = JackTx<TxCounter>>,
        outputs: impl IntoIterator<Item = JackRx<RxCounter>>,
    ) -> Self {
        Self {
            txs: inputs.into_iter().collect(),
            rxs: outputs.into_iter().collect(),
            start_frame_idx: cell::OnceCell::new(),
        }
    }
}

impl<RxCounter: Send + utils::queue::Counter, TxCounter: Send + utils::queue::Counter>
    jack::ProcessHandler for DuplexProcessHandler<TxCounter, RxCounter>
{
    /// Main JACK audio callback.
    /// 
    /// Senders read from JACK inputs and push samples into queues.
    /// Receiver pull samples from queues and write them to JACK outputs.
    fn process(&mut self, _client: &jack::Client, scope: &jack::ProcessScope) -> jack::Control {
        // Beware: at the time of writing, in Pipewire's JACK shim, this
        // counter is completely unreliable, (can decrease or jump randomly)
        let this_cycle_frame_idx = u64::from(scope.last_frame_time());
        let &first_cycle_frame_idx = self.start_frame_idx.get_or_init(|| this_cycle_frame_idx);

        let frame_idx = this_cycle_frame_idx.strict_sub(first_cycle_frame_idx);

        for JackTx { tx, interleaver } in self.txs.iter_mut() {
            let spl_idx = frame_idx.strict_mul(interleaver.n_ports().get().try_into().unwrap());
            tx.send(spl_idx, interleaver.interleave(scope).copied(), || 0.);
        }

        for JackRx { rx, interleaver } in &mut self.rxs {
            let spl_idx = frame_idx.strict_mul(interleaver.n_ports().get().try_into().unwrap());
            for (dest, src) in interleaver.interleave(scope).zip(rx.recv(spl_idx, || 0.)) {
                *dest = src
            }
        }

        jack::Control::Continue
    }

    /// Called when the JACK buffer size changes.
    ///
    /// The current implementation ignores this event, but a real system
    /// would typically tear down and rebuild internal buffering to
    /// accommodate the new size.
    fn buffer_size(&mut self, _: &jack::Client, _size: jack::Frames) -> jack::Control {
        jack::Control::Continue
    }
}
