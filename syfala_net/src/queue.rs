use super::*;

/// Sends audio data over a ring buffer, with an internal sample timer to track missed samples.
///
/// Note that everything here is in samples, for multichannel data, some extra bookkeeping
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
    ///
    /// # Panics
    ///
    /// It is a logic error for `ExactSizeIterator` to return less (or more) elements than indicated
    /// by its `len()` method. This function may panic if such is the case.
    #[inline]
    pub fn send(
        &mut self,
        timestamp: u64,
        mut samples: impl Iterator<Item = Sample>,
    ) -> Result<usize, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?;

        let chunk = self
            .tx
            .write_chunk_uninit(self.tx.slots())
            .unwrap();

        let written_samples = chunk.fill_from_iter(reshape_iter(drift, &mut samples));

        self.timer.advance_timer(written_samples);

        Ok(written_samples)
    }
}

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
    /// pad with silence, or skip samples when necessary. Use `self.an_available_samples()` to
    /// empty the ring buffer.
    #[inline]
    pub fn recv(
        &mut self,
        timestamp: u64,
        nominal_n_samples: usize,
    ) -> Result<impl Iterator<Item = Sample>, num::TryFromIntError> {
        let drift = self.timer.drift(timestamp)?;

        let n_requested_frames = drift
            .map(|drift| drift.total_samples(nominal_n_samples))
            .unwrap_or(nominal_n_samples);

        let chunk = self
            .rx
            .read_chunk(n_requested_frames.min(self.n_available_samples()))
            .unwrap();

        Ok(reshape_iter(drift, chunk))
    }
}
