//! Utilities for building predictable, low-latency, allocation-conscious data pipelines.
//!
//! This crate provides lightweight primitives for real-time data processing,
//! particularly in networking and audio systems.
//!
//! Its components are designed to compose cleanly, enabling efficient handling
//! of uninitialized buffers, periodic actions, and contiguous access to
//! segmented queues while integrating with standard I/O abstractions.

// TODO: create common adapters for [AudioData<'a> sequence] -> [padded samples]
// for different sample types
use core::mem;
pub mod queue;

/// A lightweight wrapper around [`std::time::Instant`] used to track timeouts.
/// Stores the instant at which the timer was last reset.
///
/// This type is primarily intended for implementing heartbeat or inactivity
/// timeouts in connection-like protocols built on top of UDP or other
/// connectionless transports.
///
/// ```ignore
/// let mut timer = ConnectionTimer::new();
/// 
/// // we have received a message from the peer.
/// timer.reset();
///
/// if timer.elapsed() > TIMEOUT {
///     // consider the peer disconnected
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ConnectionTimer(std::time::Instant);

impl Default for ConnectionTimer {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl ConnectionTimer {
    /// Creates a new timer starting at the current instant.
    ///
    /// Equivalent to calling [`Default::default`], but provided explicitly
    /// for clarity at call sites.
    #[inline(always)]
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }

    /// Resets the timer to start measuring elapsed time from now.
    ///
    /// This discards any previously accumulated elapsed duration.
    #[inline(always)]
    pub fn reset(&mut self) {
        *self = Self::new()
    }

    /// Returns the amount of time elapsed since the last reset.
    #[inline(always)]
    pub fn elapsed(&self) -> core::time::Duration {
        self.0.elapsed()
    }
}

/// A cursor-like writer over a buffer of uninitialized memory.
///
/// This type is conceptually similar to [`std::io::Cursor`], but operates over
/// a slice of [`core::mem::MaybeUninit<u8>`] instead of a fully initialized
/// byte slice.
///
/// # Purpose
/// 
/// This allows writing into preallocated but uninitialized memory without
/// incurring the cost of initialization, useful for
/// high-performance or low-level serialization code.
pub struct UninitCursor<'a> {
    storage: &'a mut [core::mem::MaybeUninit<u8>],
    pos: usize,
    // invariants, pos <= storage.len()
    //             the first pos bytes in storage have been initialized
}

impl<'a> UninitCursor<'a> {

    /// Returns the number of bytes, from the start of the buffer, have been initialized.
    /// any bytes beyond that may be uninitialized.
    /// 
    /// This value never decreases.
    pub const fn initialized(&self) -> usize {
        self.pos
    }

    /// Returns the underlying full buffer backing this cursor.
    ///
    /// The returned slice includes **both initialized and uninitialized**
    /// memory. It is undefined behavior to assume that any portion of the buffer
    /// beyond the current cursor position is initialized.
    /// 
    /// Primarily intended for low-level inspection,
    /// manual initialization, or usage with APIs that operate
    /// directly on `MaybeUninit<u8>` buffers.
    pub const fn inner(&self) -> &[core::mem::MaybeUninit<u8>] {
        self.storage
    }

    /// Returns a mutable reference to the underlying buffer backing this cursor.
    ///
    /// The returned slice includes **both initialized and uninitialized** memory.
    /// It is undefined behavior to assume that any portion of the buffer
    /// beyond the current cursor position is initialized.
    /// 
    /// Primarily intended for low-level inspection,
    /// manual initialization, or usage with APIs that operate
    /// directly on `MaybeUninit<u8>` buffers.
    pub const fn inner_mut(&mut self) -> &mut [core::mem::MaybeUninit<u8>] {
        self.storage
    }

    /// Creates a new cursor over an uninitialized byte buffer.
    ///
    /// Initially, no bytes are considered initialized.
    pub const fn new(storage: &'a mut [core::mem::MaybeUninit<u8>]) -> Self {
        Self { storage, pos: 0 }
    }

    /// Safely splits the buffer into initialized and uninitialized regions.
    pub const fn split(&self) -> (&[u8], &[core::mem::MaybeUninit<u8>]) {
        // SAFETY: self.pos is always <= self.storage.len()
        let (init, uninit) = unsafe { self.storage.split_at_unchecked(self.pos) };

        // SAFETY: we have initialized the first self.pos bytes in self.storage
        // NIGHTLY: #[feature(maybe_uninit_slice)] use assume_init_ref
        (unsafe { mem::transmute(init) }, uninit)
    }

    /// Safely, and mutably, splits the buffer into initialized and uninitialized regions
    /// 
    /// the `u8` slice contains elements that are guaranteed to be initialized.
    pub const fn split_mut(&mut self) -> (&mut [u8], &mut [core::mem::MaybeUninit<u8>]) {
        // Safety arguments are the same as above

        let (init, uninit) = unsafe { self.storage.split_at_mut_unchecked(self.pos) };

        // NIGHTLY: #[feature(maybe_uninit_slice)] use assume_init_mut
        (unsafe { mem::transmute(init) }, uninit)
    }
}

impl<'a> std::io::Write for UninitCursor<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {

        let remaining = self.inner().len().strict_sub(self.pos);
        let to_write = remaining.min(buf.len());

        let (_init, uninit) = self.split_mut();

        // NIGHTLY: #[feature(maybe_uninit_slice)] use write_copy_of_slice
        for (dest, &src) in core::iter::zip(uninit, buf) {
            dest.write(src);
        }

        self.pos = self.pos.strict_add(to_write);

        // Returns Ok(0) when:
        // - `buf` is empty
        // - the cursor has no remaining capacity
        Ok(to_write)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A [`std::io::Write`] adapter that chains two writers together.
///
/// Writes are first attempted on the first writer until it can no longer
/// accept data, after which all remaining writes are forwarded to the
/// second writer.
///
/// # Intended use
/// This is primarily useful for treating two fixed-size in-memory buffers
/// as a single contiguous output destination without copying.
pub struct ChainedWriter<W1, W2> {
    first: W1,
    second: W2,
    using_first: bool,
}

impl<W1, W2> ChainedWriter<W1, W2> {
    /// Creates a new chained writer from two underlying writers.
    ///
    /// The first writer will be used until it is exhausted, after which
    /// writes are forwarded to the second writer.
    #[inline(always)]
    pub const fn new(first: W1, second: W2) -> Self {
        Self {
            first,
            second,
            using_first: true,
        }
    }
}

impl<W1: std::io::Write, W2: std::io::Write> std::io::Write for ChainedWriter<W1, W2> {
    /// Attempts to write into this chained writer.
    ///
    /// - The first writer is used until it returns `Err(WriteZero)` or accepts fewer
    ///   bytes than requested.
    /// 
    /// - All subsequent writes go directly to the second writer
    fn write(&mut self, mut buf: &[u8]) -> std::io::Result<usize> {

        let mut total = 0;

        if self.using_first {

            match self.first.write(buf) {
                Ok(n) => {
                    total = n.strict_add(total);
                    buf = &buf[n..];

                    if !buf.is_empty() {
                        self.using_first = false;
                    } else {
                        return Ok(total);
                    }
                },
                // A WriteZero error indicates the first writer is exhausted
                Err(e) if e.kind() == std::io::ErrorKind::WriteZero => {
                    self.using_first = false;
                }
                Err(e) => return Err(e),
            }
        }

        if !buf.is_empty() {
            let n = self.second.write(buf)?;
            total = n.strict_add(total);
        }

        // Returns Ok(0) when:
        // - `buf` is empty
        // - both writers are unable to accept additional data
        Ok(total)
    }

    /// - `flush()` flushes both writers (if applicable)
    fn flush(&mut self) -> std::io::Result<()> {
        if self.using_first {
            self.first.flush()?;
        }
        self.second.flush()
    }
}