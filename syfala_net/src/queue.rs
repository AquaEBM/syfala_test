//! Utilities for sending audio streams to other threads using real-time ring buffers.
//! With periodic waking functionality,
//!
//! Re-exports [`rtrb`] for convenience.
use super::*;

/// Convenience re-export of rtrb
pub use rtrb;

use core::iter;

/// Sends audio data over a ring buffer, with an internal sample timer to track missed samples.
///
/// Note that everything here is in __samples__, for multichannel data, some extra bookkeeping
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
        self.timer.waker()
    }

    #[inline(always)]
    pub fn waker_mut(&mut self) -> &mut Waker {
        self.timer.waker_mut()
    }

    /// Writes the elements in `samples` into the sender's ring buffer, `timestamp` is used to
    /// pad with silence, or skip samples when necessary.
    #[inline]
    pub fn send(
        &mut self,
        timestamp: u64,
        in_samples: impl IntoIterator<Item = Sample>,
    ) -> Result<usize, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?;

        let mut n_in_samples_skipped = 0;
        let mut n_out_samples_skipped = 0;

        if let Some(drift) = drift {
            if drift.is_negative() {
                n_in_samples_skipped = drift.abs().get();
            } else {
                n_out_samples_skipped = drift.abs().get();
            }
        }

        let n_available_slots = self.available_samples();

        let mut n_pushed_samples = n_out_samples_skipped.min(n_available_slots);

        let mut chunk = self.tx.write_chunk_uninit(n_available_slots).unwrap();
        let (start, end) = chunk.as_mut_slices();

        let out_samples = iter::chain(start, end);

        for (out_sample, in_sample) in iter::zip(
            out_samples.into_iter().skip(n_out_samples_skipped),
            in_samples.into_iter().skip(n_in_samples_skipped),
        ) {
            out_sample.write(in_sample);
            n_pushed_samples += 1;
        }

        // SAFETY: Typically, or at least according to the docs, the safety argument here should
        // be the fact that we have correctly initialized the first n_pushed_samples values. We
        // _have not_. But, this is still ok because all bit patterns for f32 are valid.å
        unsafe { chunk.commit(n_pushed_samples) }

        self.timer.advance_timer(n_pushed_samples);

        Ok(n_pushed_samples)
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
        self.timer.waker()
    }

    #[inline(always)]
    pub fn waker_mut(&mut self) -> &mut Waker {
        self.timer.waker_mut()
    }

    /// Attempts to read `nominal_n_samples` samples from the ring buffer, `timestamp` is used to
    /// pad with silence, or skip samples when necessary.
    #[inline]
    pub fn recv<'a>(
        &'a mut self,
        timestamp: u64,
        out_samples: impl IntoIterator<Item = &'a mut f32>,
    ) -> Result<usize, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?;

        // notice how neither are positive at the same time
        let mut n_out_samples_skipped = 0;
        let mut n_in_samples_skipped = 0;

        if let Some(drift) = drift {
            if drift.is_negative() {
                n_out_samples_skipped = drift.abs().get();
            } else {
                n_in_samples_skipped = drift.abs().get();
            }
        }

        let n_available_slots = self.n_available_samples();

        let mut n_popped_samples = n_in_samples_skipped.min(n_available_slots);

        let in_samples = self.rx.read_chunk(n_available_slots).unwrap();

        for (out_sample, in_sample) in iter::zip(
            out_samples.into_iter().skip(n_out_samples_skipped),
            in_samples.into_iter().skip(n_in_samples_skipped),
        ) {
            *out_sample = in_sample;
            n_popped_samples = n_popped_samples.strict_add(1);
        }

        self.timer.advance_timer(n_popped_samples);

        Ok(n_popped_samples)
    }
}
