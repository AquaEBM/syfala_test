use core::cell;

pub use syfala_network as network;
pub use syfala_utils as utils;

mod interleaver;

/// The only format supported by JACK
pub const JACK_SAMPLE_TYPE: network::proto::format::SampleType =
    network::proto::format::SampleType::IEEF32;

pub type JackSample = f32;

pub struct JackTx<C> {
    interleaver: Box<interleaver::Interleaver<jack::AudioIn>>,
    tx: utils::queue::IndexedTx<C, JackSample>,
}

impl<C> JackTx<C> {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioIn>>,
        tx: utils::queue::IndexedTx<C, JackSample>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { interleaver, tx })
    }
}

pub struct JackRx<C> {
    rx: utils::queue::IndexedRx<C, JackSample>,
    interleaver: Box<interleaver::Interleaver<jack::AudioOut>>,
}

impl<C> JackRx<C> {
    pub fn new(
        ports: impl IntoIterator<Item = jack::Port<jack::AudioOut>>,
        rx: utils::queue::IndexedRx<C, JackSample>,
    ) -> Option<Self> {
        let interleaver = interleaver::Interleaver::new(ports)?;

        Some(Self { rx, interleaver })
    }
}

pub struct DuplexProcessHandler<C> {
    txs: Box<[JackTx<C>]>,
    rxs: Box<[JackRx<C>]>,
    start_frame_idx: cell::OnceCell<u64>,
}

impl<C> DuplexProcessHandler<C> {
    #[inline(always)]
    pub fn new(
        inputs: impl IntoIterator<Item = JackTx<C>>,
        outputs: impl IntoIterator<Item = JackRx<C>>,
    ) -> Self {
        Self {
            txs: inputs.into_iter().collect(),
            rxs: outputs.into_iter().collect(),
            start_frame_idx: cell::OnceCell::new(),
        }
    }
}

impl<C: Send + utils::queue::Counter> jack::ProcessHandler for DuplexProcessHandler<C> {
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

    // TODO: do something here that disconnects from all clients, and restarts with a new buffer size
    fn buffer_size(&mut self, _: &jack::Client, _size: jack::Frames) -> jack::Control {
        jack::Control::Continue
    }
}
