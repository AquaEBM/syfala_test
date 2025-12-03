//! Utilities for sending audio streams to other threads using real-time ring buffers.
//! With periodic waking functionality,
//! 
//! Re-exports [`rtrb`] for convenience.

use super::*;

/// Convenience re-export of rtrb
pub use rtrb;

/// Sends audio data over a ring buffer, with an internal sample timer to track missed samples.
///
/// Note that everything here is in _samples_, for multichannel data, some extra bookkeeping
/// might be needed.
pub struct Sender {
    tx: rtrb::Producer<Sample>,
    timer: timing::WakingTimer,
}

impl Sender {
    #[inline(always)]
    pub fn new(tx: rtrb::Producer<Sample>) -> Self {
        Self {
            tx,
            timer: timing::WakingTimer::default(),
        }
    }

    #[inline(always)]
    pub fn with_waker(tx: rtrb::Producer<Sample>, waker: Waker) -> Self {
        Self {
            tx,
            timer: timing::WakingTimer::with_waker(waker),
        }
    }

    #[inline(always)]
    pub const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.timer.set_zero_timestamp(timestamp);
    }

    #[inline(always)]
    pub fn is_abandoned(&self) -> bool {
        self.tx.is_abandoned()
    }

    #[inline(always)]
    pub fn capacity_samples(&self) -> usize {
        self.tx.buffer().capacity()
    }

    #[inline(always)]
    pub fn available_samples(&self) -> usize {
        self.tx.slots()
    }

    #[inline(always)]
    pub fn waker(&self) -> &Waker {
        &self.timer.waker
    }

    #[inline(always)]
    pub fn waker_mut(&mut self) -> &mut Waker {
        &mut self.timer.waker
    }

    /// Writes the elements in `samples` into the sender's ring buffer, `timestamp` is used to
    /// pad with silence, or skip samples when necessary.
    #[inline]
    pub fn send(
        &mut self,
        timestamp: u64,
        samples: impl Iterator<Item = Sample>,
    ) -> Result<usize, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?;

        let available_slots = self.tx.slots();

        let chunk = self.tx.write_chunk_uninit(available_slots).unwrap();

        let written_samples = chunk.fill_from_iter(reshape_iter(drift, samples));

        self.timer.advance_timer(written_samples);

        Ok(written_samples)
    }
}

/// Sends audio data from a ring buffer, with an internal sample timer to track missed samples.
///
/// Note that everything here is in _samples_, for multichannel data, some extra bookkeeping
/// might be needed.
pub struct Receiver {
    rx: rtrb::Consumer<Sample>,
    timer: timing::WakingTimer,
}

impl Receiver {
    #[inline(always)]
    pub fn new(rx: rtrb::Consumer<Sample>) -> Self {
        Self {
            rx,
            timer: timing::WakingTimer::default(),
        }
    }

    #[inline(always)]
    pub fn with_waker(rx: rtrb::Consumer<Sample>, waker: Waker) -> Self {
        Self {
            rx,
            timer: timing::WakingTimer::with_waker(waker),
        }
    }

    #[inline(always)]
    pub const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.timer.set_zero_timestamp(timestamp);
    }

    #[inline(always)]
    pub fn is_abandoned(&self) -> bool {
        self.rx.is_abandoned()
    }

    #[inline(always)]
    pub fn capacity_samples(&self) -> usize {
        self.rx.buffer().capacity()
    }

    #[inline(always)]
    pub fn n_available_samples(&self) -> usize {
        self.rx.slots()
    }

    #[inline(always)]
    pub fn waker(&self) -> &Waker {
        &self.timer.waker
    }

    #[inline(always)]
    pub fn waker_mut(&mut self) -> &mut Waker {
        &mut self.timer.waker
    }

    /// Attempts to read `nominal_n_samples` samples from the ring buffer, `timestamp` is used to
    /// pad with silence, or skip samples when necessary.
    #[inline]
    pub fn recv<'a>(
        &'a mut self,
        timestamp: u64,
        outputs: impl IntoIterator<Item = &'a mut f32>,
    ) -> Result<usize, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?.map(ops::Neg::neg);

        let available = self.n_available_samples();

        let chunk = self.rx.read_chunk(available).unwrap();

        let written_samples = iter::zip(
            reshape_iter(drift, chunk),
            outputs,
        )
        .map(|(in_sample, out_sample)| *out_sample = in_sample)
        .count();

        self.timer.advance_timer(written_samples);

        Ok(written_samples)
    }
}
