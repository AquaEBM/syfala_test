///! Common utilities for interacting with queues. Used throughout implementations of our protocol.

use core::num;

pub use rtrb;

/// A counter that keeps track of if (or how many times) it completes a chunk.
#[derive(Debug, Clone, Copy)]
pub struct PeriodicCounter {
    period: num::NonZeroUsize,
    current: usize, // always less than self.period
}

impl PeriodicCounter {
    #[inline(always)]
    pub const fn new(period: num::NonZeroUsize) -> Self {
        Self {
            period,
            current: 0,
        }
    }

    /// Returns the period size, or chunk size of this counter.
    #[inline(always)]
    pub const fn period(&self) -> num::NonZeroUsize {
        self.period
    }

    /// Advances the counter by `n`
    /// 
    /// Returns the number of chunk boundaries we have stepped through.
    #[inline(always)]
    pub fn advance(&mut self, n: usize) -> usize {
        let p = self.period();
        let next_chunk_idx_non_wrapped = self.current.strict_add(n);
        self.current = next_chunk_idx_non_wrapped % p;
        let boundaries = next_chunk_idx_non_wrapped / p;

        boundaries
    }
}

/// Aggregates the producer side of a ring buffer, and a periodic counter,
/// enabling the user to (e.g.) periodically wake a thread when engough data is sent to it.
#[derive(Debug)]
pub struct PeriodicWakingTx<T> {
    pub tx: rtrb::Producer<T>,
    pub counter: PeriodicCounter,
}

impl<T> PeriodicWakingTx<T> {
    #[inline(always)]
    pub const fn new(
        tx: rtrb::Producer<T>,
        waking_period: num::NonZeroUsize,
    ) -> Self {
        Self {
            tx,
            counter: PeriodicCounter::new(waking_period),
        }
    }
}
