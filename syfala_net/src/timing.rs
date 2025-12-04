use super::*;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Timer {
    zero: u64,
    curr: u64,
    // invariant: current >= zero
}

impl Default for Timer {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    #[inline(always)]
    const fn new() -> Self {
        Self { zero: 0, curr: 0 }
    }

    #[inline(always)]
    const fn elapsed(&self) -> u64 {
        self.curr.strict_sub(self.zero)
    }

    #[inline(always)]
    const fn current(&self) -> u64 {
        self.curr
    }

    #[inline(always)]
    const fn zero(&self) -> u64 {
        self.zero
    }

    #[inline(always)]
    fn chunk_time(&self, chunk_size: num::NonZeroUsize) -> usize {
        // elapsed % chunk_size
        let elapsed = self.elapsed();
        usize::try_from(elapsed % num::NonZeroU64::try_from(chunk_size).unwrap()).unwrap()
    }

    #[inline(always)]
    fn remaining_chunk_time(&self, chunk_size: num::NonZeroUsize) -> num::NonZeroUsize {
        // chunk_size - elapsed % chunk_size

        num::NonZeroUsize::new(chunk_size.get().strict_sub(self.chunk_time(chunk_size))).unwrap()
    }

    #[inline(always)]
    fn advance(&mut self, time: usize) {
        self.curr = self.curr.strict_add(u64::try_from(time).unwrap());
    }

    #[inline(always)]
    const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.curr = if timestamp < self.zero() {
            self.curr.strict_sub(self.zero().strict_sub(timestamp))
        } else {
            self.curr.strict_add(timestamp.strict_sub(self.zero()))
        };
        self.zero = timestamp;
    }

    #[inline(always)]
    fn drift(&self, next_timestamp: u64) -> Result<Option<Drift>, num::TryFromIntError> {
        Drift::new(self, next_timestamp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Drift {
    negative: bool,
    abs: num::NonZeroUsize,
}

impl Drift {
    #[inline(always)]
    fn new(timer: &Timer, next_timestamp: u64) -> Result<Option<Self>, num::TryFromIntError> {
        let c = timer.current();
        usize::try_from(next_timestamp.abs_diff(c)).map(|abs| {
            num::NonZeroUsize::new(abs).map(|abs| Self {
                negative: next_timestamp < c,
                abs,
            })
        })
    }

    #[inline(always)]
    pub(crate) const fn is_negative(&self) -> bool {
        self.negative
    }

    #[inline(always)]
    pub(crate) const fn abs(&self) -> num::NonZeroUsize {
        self.abs
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WakingTimer {
    timer: timing::Timer,
    waker: Waker,
}

impl Default for WakingTimer {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl WakingTimer {
    #[inline(always)]
    pub(crate) fn new() -> Self {
        Self::with_waker(Waker::useless())
    }

    #[inline(always)]
    pub(crate) const fn with_waker(waker: Waker) -> Self {
        Self {
            timer: timing::Timer::new(),
            waker,
        }
    }

    #[inline(always)]
    pub(crate) fn advance_timer(&mut self, delta: usize) {
        let rem_spls = self
            .timer
            .remaining_chunk_time(self.waker.chunk_size_samples());
        if delta >= rem_spls.get() {
            self.waker.wake();
        }

        self.timer.advance(delta);
    }

    #[inline(always)]
    pub(crate) const fn waker(&self) -> &Waker {
        &self.waker
    }

    #[inline(always)]
    pub(crate) const fn waker_mut(&mut self) -> &mut Waker {
        &mut self.waker
    }

    #[inline(always)]
    pub(crate) const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.timer.set_zero_timestamp(timestamp);
    }

    #[inline(always)]
    pub(crate) fn drift(&self, next_timestamp: u64) -> Result<Option<Drift>, num::TryFromIntError> {
        self.timer.drift(next_timestamp)
    }
}
