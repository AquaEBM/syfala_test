//! Common queue-related utilities used throughout the protocol implementation.
//! 
//! This module provides small helpers and adapters for working with ring
//! buffers, byte I/O on ring buffers, and periodic wake-up logic.
use core::num;

pub use rtrb;

/// A counter that tracks progress through fixed-size periods.
///
/// Each time the counter advances past a multiple of it's period, a boundary
/// is considered crossed.
#[derive(Debug, Clone, Copy)]
pub struct PeriodicCounter {
    period: num::NonZeroUsize,
    current: usize, // always less than self.period
}

impl PeriodicCounter {
    /// Creates a new counter with the given `period`.
    #[inline(always)]
    pub const fn new(period: num::NonZeroUsize) -> Self {
        Self { period, current: 0 }
    }

    /// Returns the configured period (or chunk size).
    #[inline(always)]
    pub const fn period(&self) -> num::NonZeroUsize {
        self.period
    }

    /// Advances the counter by `n` steps.
    ///
    /// Returns the number of period boundaries crossed.
    #[inline(always)]
    pub fn advance(&mut self, n: usize) -> usize {
        let p = self.period();
        let next_chunk_idx_non_wrapped = self.current.strict_add(n);
        self.current = next_chunk_idx_non_wrapped % p;
        let boundaries = next_chunk_idx_non_wrapped / p;

        boundaries
    }
}

/// Combines a ring-buffer producer with a periodic counter.
///
/// This is useful for producer-side logic that needs to trigger an action
/// (such as waking another thread) after a fixed amount of data is written.
#[derive(Debug)]
pub struct PeriodicWakingTx<T> {
    pub tx: rtrb::Producer<T>,
    pub counter: PeriodicCounter,
}

impl<T> PeriodicWakingTx<T> {
    /// Creates a new producer with an associated waking period.
    #[inline(always)]
    pub const fn new(tx: rtrb::Producer<T>, waking_period: num::NonZeroUsize) -> Self {
        Self {
            tx,
            counter: PeriodicCounter::new(waking_period),
        }
    }
}

/// Acquires a write chunk covering all available producer slots.
#[inline(always)]
pub fn producer_get_all<T>(tx: &mut rtrb::Producer<T>) -> rtrb::chunks::WriteChunkUninit<'_, T> {
    tx.write_chunk_uninit(tx.slots()).unwrap()
}

/// Acquires a read chunk covering all available consumer slots.
#[inline(always)]
pub fn consumer_get_all<T>(rx: &mut rtrb::Consumer<T>) -> rtrb::chunks::ReadChunk<'_, T> {
    rx.read_chunk(rx.slots()).unwrap()
}

/// Returns a writer that writes contiguously across a split write chunk.
/// 
/// Internally chains two cursors to present the chunk as a single
/// `std::io::Write` implementation.
#[inline(always)]
pub fn chunk_get_writer(
    chunk: &mut rtrb::chunks::WriteChunkUninit<u8>,
) -> impl std::io::Write {
    let (start, end) = chunk.as_mut_slices();

    crate::ChainedWriter::new(crate::UninitCursor::new(start), crate::UninitCursor::new(end))
}

/// Returns a reader that reads contiguously across a split read chunk.
/// 
/// Internally chains two cursors to present the chunk as a single
/// `std::io::Read` implementation.
#[inline(always)]
pub fn chunk_get_reader(
    chunk: &mut rtrb::chunks::ReadChunk<u8>,
) -> impl std::io::Read {
    let (start, end) = chunk.as_slices();

    std::io::Read::chain(std::io::Cursor::new(start), std::io::Cursor::new(end))
}