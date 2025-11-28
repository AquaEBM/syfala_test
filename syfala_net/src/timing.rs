use super::*;
use core::{mem, ops};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Timer {
    zero: u64,
    current: u64,
    // invariant: current >= zero
}

impl Timer {
    #[inline(always)]
    const fn new() -> Self {
        Self {
            zero: 0,
            current: 0,
        }
    }

    #[inline(always)]
    const fn elapsed(&self) -> u64 {
        self.current.strict_sub(self.zero)
    }

    #[inline(always)]
    const fn current(&self) -> u64 {
        self.current
    }

    #[inline(always)]
    const fn zero(&self) -> u64 {
        self.zero
    }

    #[inline(always)]
    fn chunk_time(&self, chunk_size: num::NonZeroUsize) -> usize {
        // elapsed % chunk_size
        usize::try_from(self.elapsed() % num::NonZeroU64::try_from(chunk_size).unwrap()).unwrap()
    }

    #[inline(always)]
    fn remaining_chunk_time(&self, chunk_size: num::NonZeroUsize) -> num::NonZeroUsize {
        // chunk_size - elapsed % chunk_size

        num::NonZeroUsize::new(chunk_size.get().strict_sub(self.chunk_time(chunk_size))).unwrap()
    }

    #[inline(always)]
    fn advance(&mut self, time: usize) {
        self.current = self.current.strict_add(u64::try_from(time).unwrap());
    }

    #[inline(always)]
    const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.current = if timestamp < self.zero() {
            self.current.strict_sub(self.zero().strict_sub(timestamp))
        } else {
            self.current.strict_add(timestamp.strict_sub(self.zero()))
        };
        self.zero = timestamp;
    }

    #[inline(always)]
    fn drift(&self, next_timestamp: u64) -> Option<Drift> {
        Drift::new(self, next_timestamp)
    }
}

impl Default for Timer {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl ops::AddAssign<usize> for Timer {
    #[inline(always)]
    fn add_assign(&mut self, rhs: usize) {
        self.advance(rhs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Drift {
    negative: bool,
    abs: num::NonZeroUsize,
}

impl Drift {
    #[inline(always)]
    fn new(timer: &Timer, next_timestamp: u64) -> Option<Self> {
        let c = timer.current();
        num::NonZeroUsize::new(usize::try_from(next_timestamp.abs_diff(c)).unwrap()).map(|abs| {
            Self {
                negative: next_timestamp < c,
                abs,
            }
        })
    }

    #[inline(always)]
    pub(crate) fn total_samples(self, nominal_n_samples: usize) -> usize {
        let abs = num::NonZeroUsize::try_from(self.abs).unwrap();

        if self.negative {
            nominal_n_samples.saturating_sub(abs.get())
        } else {
            nominal_n_samples.strict_add(abs.get())
        }
    }

    #[inline(always)]
    pub const fn is_negative(&self) -> bool {
        self.negative
    }

    #[inline(always)]
    pub const fn abs(&self) -> num::NonZeroUsize {
        self.abs
    }
}

#[derive(Clone)]
pub struct Waker {
    thread_handle: thread::Thread,
    chunk_size_spls: num::NonZeroUsize,
}

impl Waker {
    #[inline(always)]
    pub fn useless() -> Self {
        Self::new(thread::current(), num::NonZeroUsize::MAX)
    }

    #[inline(always)]
    pub const fn new(thread_handle: thread::Thread, chunk_size_spls: num::NonZeroUsize) -> Self {
        Self {
            thread_handle,
            chunk_size_spls,
        }
    }

    #[inline(always)]
    pub const fn chunk_size_samples(&self) -> num::NonZeroUsize {
        self.chunk_size_spls
    }

    #[inline(always)]
    pub const fn set_chunk_size_samples(&mut self, chunk_size_spls: num::NonZeroUsize) {
        self.chunk_size_spls = chunk_size_spls;
    }

    #[inline(always)]
    pub const fn set_thread_handle(&mut self, handle: thread::Thread) -> thread::Thread {
        mem::replace(&mut self.thread_handle, handle)
    }

    #[inline(always)]
    fn wake(&self) {
        self.thread_handle.unpark();
    }
}

pub(crate) struct WakingTimer {
    timer: timing::Timer,
    pub(crate) waker: Waker,
}

impl Default for WakingTimer {
    #[inline(always)]
    fn default() -> Self {
        Self {
            timer: timing::Timer::new(),
            waker: Waker::useless(),
        }
    }
}

impl WakingTimer {
    #[inline(always)]
    pub(crate) const fn new(waker: Waker) -> Self {
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
        if delta <= rem_spls.get() {
            self.waker.wake();
        }

        self.timer.advance(delta);
    }

    #[inline(always)]
    pub(crate) const fn set_zero_timestamp(&mut self, timestamp: u64) {
        self.timer.set_zero_timestamp(timestamp);
    }

    #[inline(always)]
    pub(crate) fn drift(&self, next_timestamp: u64) -> Option<Drift> {
        self.timer.drift(next_timestamp)
    }
}
