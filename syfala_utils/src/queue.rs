//! Common queue-related utilities used to transport streams between threads.
//! 
//! This module provides small helpers and adapters for working with ring
//! buffers and periodic wake-up logic. It also provides ring buffer adapters
//! that track and automatically react (by padding/skipping samples) to data misalignment
//! (audio cycle skips, packet loss, packet reordering, jitter...)
use core::{num, iter};

pub use rtrb;
/// A minimal abstraction for a monotonically increasing logical counter.
/// 
/// This trait is intentionally minimal, allowing implementations to layer
/// additional behavior.
pub trait Counter {
    /// Advances the counter by `delta` steps.
    /// 
    /// Implementations may perform additional side effects, such as
    /// waking other threads or tracking boundaries.
    fn advance(&mut self, delta: usize);

    /// Returns the current absolute position of the counter.
    /// 
    /// The returned value is expected to remain unchanged until the next call to
    /// [`self.advance(delta)`](Counter::advance), which is, then, expected to increase by
    /// _exactly_ `delta`
    fn current(&self) -> u64;
}

/// Blanket implementation of [`Counter`] for mutable references.
///
/// This allows `&mut T` to be passed wherever a [`Counter`] is expected,
/// without forcing callers to manually dereference.
impl<'a, T: Counter> Counter for &'a mut T {
    #[inline(always)]
    fn advance(&mut self, delta: usize) {
        (**self).advance(delta);
    }

    #[inline(always)]
    fn current(&self) -> u64 {
        (**self).current()
    }
}

/// A simple, standalone counter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GenericCounter(u64);

impl GenericCounter {
    /// Creates a new counter initialized to zero.
    #[inline(always)]
    pub const fn new() -> Self {
        Self(0)
    }
}

impl Counter for GenericCounter {
    #[inline(always)]
    fn advance(&mut self, delta: usize) {
        self.0 = self.0.strict_add(delta.try_into().unwrap())
    }

    #[inline(always)]
    fn current(&self) -> u64 {
        self.0
    }
}

/// Trait encapsulating the ability to wake or notify another execution context.
///
/// This abstraction models behavior similar to a counting semaphore:
/// calling [`Waker::wake`] signals that some amount of work has become
/// available, potentially unblocking another thread or task.
pub trait Waker {
    /// Notifies another execution context that `times` wake events occurred.
    ///
    /// The semantics of multiple wakeups are implementation-defined, but
    /// callers should assume that each wake corresponds to one unit of
    /// newly available work.
    fn wake(&mut self, times: num::NonZeroUsize);
}

/// No-op `Waker` implementation for the unit type.
///
/// This is useful when wake-up behavior is optional or undesirable,
/// allowing counters to be used without introducing conditional logic.
impl Waker for () {
    #[inline(always)]
    fn wake(&mut self, _times: num::NonZeroUsize) {}
}

#[cfg(feature = "std")]
/// `Waker` implementation for standard OS threads.
///
/// Each wake operation unparks the associated thread, allowing it
/// to resume execution if it was previously parked.
impl Waker for std::thread::Thread {
    #[inline(always)]
    fn wake(&mut self, _times: num::NonZeroUsize) {
        self.unpark()
    }
}

/// A counter adapter that tracks progress through fixed-size periods.
///
/// `PeriodicCounter` wraps another [`Counter`] and observes how many
/// multiples of a fixed `period` have been crossed as the counter advances.
/// Each time one or more new boundaries are crossed, a [`Waker`] is notified.
///
/// This is useful for chunked processing models where work becomes
/// available in discrete blocks (e.g. buffer sizes, in audio frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeriodicCounter<C, W> {
    counter: C,
    waker: W,
    period: num::NonZeroUsize,
}

impl<C, W> PeriodicCounter<C, W> {
    /// Creates a new periodic counter.
    ///
    /// - `period` defines the size of each logical chunk
    /// - `counter` is the underlying counter being observed
    /// - `waker` is notified when one or more boundaries are crossed
    #[inline(always)]
    pub const fn new(period: num::NonZeroUsize, counter: C, waker: W) -> Self {
        Self {
            period,
            waker,
            counter,
        }
    }

    /// Returns the configured period (chunk size).
    #[inline(always)]
    pub const fn period(&self) -> num::NonZeroUsize {
        self.period
    }
}

impl<C: Counter, W> PeriodicCounter<C, W> {
    /// Returns the total number of period boundaries crossed so far.
    ///
    /// This value is derived from the underlying counter and does not
    /// depend on how many times `advance` was called.
    #[inline(always)]
    pub fn boundaries_crossed(&self) -> u64 {
        self.counter.current() / num::NonZeroU64::try_from(self.period()).unwrap()
    }
}

impl<C: Counter, W: Waker> Counter for PeriodicCounter<C, W> {
    /// Advances the counter by `n` steps.
    ///
    /// If advancing causes one or more new period boundaries to be crossed,
    /// the associated [`Waker`] is notified with the number of newly crossed
    /// boundaries.
    #[inline(always)]
    fn advance(&mut self, n: usize) {
        let b = self.boundaries_crossed();

        self.counter.advance(n);

        if let Some(n) =
            num::NonZeroUsize::new(self.boundaries_crossed().strict_sub(b).try_into().unwrap())
        {
            self.waker.wake(n);
        }
    }

    /// Returns the current value of the underlying counter.
    #[inline(always)]
    fn current(&self) -> u64 {
        self.counter.current()
    }
}

/// Shifts an iterator forward or backward by a signed deviation.
///
/// Let `n = |deviation|`.
///
/// - If `deviation > 0`, the first `n` elements of the iterator are skipped.
/// - If `deviation < 0`, the iterator is prefixed with
///   `n` padding elements generated by `pad_fn`.
/// - If `deviation == 0`, the iterator is returned unchanged.
/// 
/// This function is primarily useful when aligning streams indexed by an
/// external counter, where missing elements must be synthesized and excess
/// elements discarded.
/// 
/// No allocations are performed.
/// 
/// Since [`Iterator`]s are lazy this function does nothing unless the returned iterator
/// is used.
#[inline(always)]
pub fn shift_iter<I: IntoIterator>(
    it: I,
    deviation: isize,
    pad_fn: impl FnMut() -> I::Item,
) -> impl IntoIterator<Item = I::Item> {
    // notice how neither are positive at the same time
    let mut padding = 0;
    let mut skipping = 0;

    *if deviation.is_negative() {
        &mut padding
    } else {
        &mut skipping
    } = deviation.unsigned_abs();

    iter::chain(
        iter::repeat_with(pad_fn).take(padding),
        it.into_iter().skip(skipping),
    )
}

/// A receive-side adapter that associates values pulled from a ring buffer
/// with a monotonically increasing external counter.
/// 
/// This allows consumers of a ring buffer to request data aligned to a specific
/// index, transparently handling gaps and overruns, lazily, and without allocations.
/// 
/// Useful for decoupling data production and consumption, handling packet loss, audio cycle
/// skips, and jitter. All while receiving data according to a stable, logical timeline.
pub struct IndexedRx<Counter, Elem> {
    rx: rtrb::Consumer<Elem>,
    counter: Counter,
}

impl<Counter, Elem> IndexedRx<Counter, Elem> {
    /// Creates a new `IndexedRx` from a ring buffer consumer and a counter.
    ///
    /// The counter is assumed to represent the logical index corresponding
    /// to the next value expected from the consumer.
    /// 
    /// Ideally, at the time of calling this function `counter.current()` should
    /// return 0. 
    #[inline(always)]
    pub fn new(rx: rtrb::Consumer<Elem>, counter: Counter) -> Self {
        Self { rx, counter }
    }
}

impl<Ctr: Counter, Elem> IndexedRx<Ctr, Elem> {
    /// Attempts to receive all currently available values from the ring buffer
    /// and align them to the requested logical index.
    ///
    /// The `idx` parameter represents the desired logical index of the first
    /// returned element. If the underlying data stream is ahead or behind
    /// this index, the returned iterator is adjusted accordingly:
    ///
    /// - If data is *ahead*, excess elements are skipped.
    /// - If data is *behind*, missing elements are synthesized using `pad_fn`.
    ///
    /// This method never blocks and performs no allocation. All adjustments
    /// are applied lazily via iterator composition.
    // TODO: we cannot implement ExactSizeIterator for this, because Chain doesn't
    // implement it for some reason, even though it's size is known.
    #[inline]
    pub fn recv(&mut self, idx: u64, pad_fn: impl FnMut() -> Elem) -> impl IntoIterator<Item = Elem> {
        let deviation: isize = idx
            .checked_signed_diff(self.counter.current())
            .unwrap()
            .try_into()
            .unwrap();

        let in_samples = consumer_get_all(&mut self.rx);
        let iter = ReadChunksIterCounter::new(in_samples, &mut self.counter);

        shift_iter(iter, deviation, pad_fn)
    }
}

/// An iterator, wrapping a [`rtrb::chunks::ReadChunkIntoIter`], that, upon destruction,
/// increments a [`Counter`] with the number of items consumed.
// TODO: I'm not sure if it's better for this to increment the counter on every iteration
// or increment everything all at once at the end (destructor)... 
struct ReadChunksIterCounter<'a, C: Counter, T> {
    ticker: C,
    iter: rtrb::chunks::ReadChunkIntoIter<'a, T>,
    // when the issue we raised with rtrb gets addressed we can remove this
    initial_len: usize,
}

impl<'a, C: Counter, T> ReadChunksIterCounter<'a, C, T> {
    #[inline(always)]
    fn new(chunk: rtrb::chunks::ReadChunk<'a, T>, ticker: C) -> Self {
        Self {
            initial_len: chunk.len(),
            iter: chunk.into_iter(),
            ticker,
        }
    }
}

impl<'a, C: Counter, T> Iterator for ReadChunksIterCounter<'a, C, T> {
    type Item = T;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }

    #[inline(always)]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.iter.nth(n)
    }
}

impl<'a, C: Counter, T> ExactSizeIterator for ReadChunksIterCounter<'a, C, T> {
    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<'a, C: Counter, T> Drop for ReadChunksIterCounter<'a, C, T> {
    #[inline(always)]
    fn drop(&mut self) {
        self.ticker
            // sent samples = initial samples - remaining samples
            .advance(self.initial_len.strict_sub(self.iter.len()));

        // i have raised an issue in the rtrb repo (https://github.com/mgeier/rtrb/issues/155)
        // when it gets fixed, and published, we can remove our initial len field
        // and just use the new method
    }
}

/// A sender-side adapter that associates values written to a ring buffer
/// with a monotonically increasing external counter.
///
/// This allows callers to submit values intended for a specific logical position
/// transparently handling gaps and overlaps.
///
/// Useful for decoupling data production and consumption, handling packet loss, audio cycle
/// skips, and jitter. All while sending data according to a stable, logical timeline.
pub struct IndexedTx<C, T> {
    counter: C,
    tx: rtrb::Producer<T>,
}

impl<C, T> IndexedTx<C, T> {
    /// Creates a new `IndexedTx` from a ring buffer producer and a counter.
    ///
    /// The counter is assumed to represent the logical index corresponding
    /// to the next element that will be written into the producer.
    #[inline(always)]
    pub const fn new(tx: rtrb::Producer<T>, counter: C) -> Self {
        Self { counter, tx }
    }
}

impl<C: Counter, T> IndexedTx<C, T> {
    /// Attempts to write the elements from the provided iterater into the ring buffer
    /// and align them to the requested logical index.
    ///
    /// The `idx` parameter represents the desired logical index of the first
    /// written element. If the underlying data stream is ahead or behind
    /// this index, the returned iterator is adjusted accordingly:
    ///
    /// - If data is *ahead*, missing elements are synthesized using `pad_fn`.
    /// - If data is *behind*, excess elements skipped.
    ///
    /// This method never blocks and performs no allocation. All adjustments
    /// are applied lazily via iterator composition.
    /// 
    /// This method will silently not write the remaining elements
    /// if the ring buffer's capacity is too small. The internal counter, however,
    /// will have kept track of the number of elements written.
    #[inline]
    pub fn send(
        &mut self,
        idx: u64,
        values: impl IntoIterator<Item = T>,
        pad_fn: impl FnMut() -> T,
    ) {
        let deviation: isize = self.counter.current()
            .checked_signed_diff(idx)
            .unwrap()
            .try_into()
            .unwrap();

        let out_iter = shift_iter(values, deviation, pad_fn);
        let n_pushed_samples = producer_get_all(&mut self.tx).fill_from_iter(out_iter);

        self.counter.advance(n_pushed_samples);
    }
    
    /// Returns the current value of the internal conter.
    #[inline(always)]
    pub fn current(&self) -> u64 {
        self.counter.current()
    }

    /// Returns the number of slots that can be written at the moment this function was called
    #[inline(always)]
    pub fn available_slots(&self) -> usize {
        self.tx.slots()
    }

    /// Returns whether the consumer side of the ring buffer has been destroyed.
    #[inline(always)]
    pub fn is_abandoned(&self) -> bool {
        self.tx.is_abandoned()
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
