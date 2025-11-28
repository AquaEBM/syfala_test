use super::*;

/// Sends audio data over a ring buffer, with an internal sample timer to track missed samples.
///
/// Note that everything here is in samples, for multichannel data, some extra bookkeeping
/// might be needed.
pub struct Sender<T> {
    tx: rtrb::Producer<T>,
    timer: timing::WakingTimer,
}

impl<T> Sender<T> {
    #[inline(always)]
    pub fn new(tx: rtrb::Producer<T>) -> Self {
        Self {
            tx,
            timer: timing::WakingTimer::default(),
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
    pub fn with_waker(tx: rtrb::Producer<T>, waker: timing::Waker) -> Self {
        Self {
            tx,
            timer: timing::WakingTimer::new(waker),
        }
    }

    #[inline(always)]
    pub fn waker(&self) -> &timing::Waker {
        &self.timer.waker
    }

    #[inline(always)]
    pub fn waker_mut(&mut self) -> &mut timing::Waker {
        &mut self.timer.waker
    }

    #[inline(always)]
    pub fn advance_timer(&mut self, n_frames: usize) {
        self.timer.advance_timer(n_frames);
    }

    /// Returns a triplet `(skipped, drift, chunk)` for the user to write data to the chunk.
    ///
    /// If `skipped`, `chunk`'s length will be at most `nominal_n_frames + drift` user is
    /// expected to write `drift` frames of silence at the beginning of `chunk`.
    ///
    /// If `!skipped`, `chunk`'s length will be at most `nominal_n_frames - drift`. The user is
    /// expected to skip the first `drift` frames they expected to write, and only write the rest.
    ///
    /// After writing to the chunk and committing it, the correct way to update the timestamp
    /// would be to add how many frames were written, e.g. `sender.advance_timer(chunk.len())`
    #[inline(always)]
    pub fn send(
        &mut self,
        timestamp: u64,
        nominal_n_samples: usize,
    ) -> (Option<timing::Drift>, rtrb::chunks::WriteChunkUninit<'_, T>) {
        let drift = self.timer.drift(timestamp);

        let n_requested_frames = drift
            .map(|drift| drift.total_samples(nominal_n_samples))
            .unwrap_or(nominal_n_samples);

        (
            drift,
            self.tx
                .write_chunk_uninit(n_requested_frames.min(self.tx.slots()))
                .unwrap(),
        )
    }
}
