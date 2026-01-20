//! Simple utilities use throughout implementations of our protocol

// TODO: create common adapters for [AudioData<'a> sequence] -> [padded samples]
// for different sample types

use core::num;
pub mod queue;

/// A small wrapper type used as a heartbeat (or any other message) timeout
/// 
/// Intended to be used to maintain UDP "connections".
#[derive(Debug, Clone)]
pub struct ConnectionTimer(std::time::Instant);

impl Default for ConnectionTimer {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl ConnectionTimer {
    /// Creates a new timer, measuring time from the moment it is created.
    #[inline(always)]
    pub fn new() -> Self {
        Self(std::time::Instant::now())
    }

    /// Resets the timer
    #[inline(always)]
    pub fn reset(&mut self) {
        *self = Self::new()
    }

    /// Measured elapsed time since the last time this timer was reset.
    #[inline(always)]
    pub fn elapsed(&self) -> core::time::Duration {
        self.0.elapsed()
    }
}

#[derive(Debug)]
pub struct MultichannelTx {
    n_channels: num::NonZeroUsize,
    // contains raw audio packets
    tx: rtrb::Producer<u8>,
}

impl MultichannelTx {
    #[inline(always)]
    pub const fn new(tx: rtrb::Producer<u8>, n_channels: num::NonZeroUsize) -> Self {
        Self { n_channels, tx }
    }

    #[inline(always)]
    pub const fn n_channels(&self) -> num::NonZeroUsize {
        self.n_channels
    }

    #[inline(always)]
    pub const fn tx(&self) -> &rtrb::Producer<u8> {
        &self.tx
    }

    #[inline(always)]
    pub const fn tx_mut(&mut self) -> &mut rtrb::Producer<u8> {
        &mut self.tx
    }
}

#[derive(Debug)]
pub struct MultichannelRx {
    n_channels: num::NonZeroUsize,
    // contains raw audio packets
    rx: rtrb::Consumer<u8>,
}

impl MultichannelRx {
    #[inline(always)]
    pub const fn new(rx: rtrb::Consumer<u8>, n_channels: num::NonZeroUsize) -> Self {
        Self { n_channels, rx }
    }

    #[inline(always)]
    pub const fn n_channels(&self) -> num::NonZeroUsize {
        self.n_channels
    }

    #[inline(always)]
    pub const fn rx(&self) -> &rtrb::Consumer<u8> {
        &self.rx
    }

    #[inline(always)]
    pub const fn tx_mut(&mut self) -> &mut rtrb::Consumer<u8> {
        &mut self.rx
    }
}

/// An implementation of [`std::io::Cursor`] that uses uninit slices.
pub struct UninitCursor<'a> {
    storage: &'a mut [core::mem::MaybeUninit<u8>],
    pos: usize,
    // invariants, pos <= storage.len()
    //             the first pos bytes in storage have been initialized
}

impl<'a> UninitCursor<'a> {
    /// Create a new cursor over the uninitialized buffer
    pub fn new(storage: &'a mut [core::mem::MaybeUninit<u8>]) -> Self {
        Self { storage, pos: 0 }
    }

    /// Return references to the initialized and uninitialized portions of our buffer
    pub fn split(&self) -> (&[u8], &[core::mem::MaybeUninit<u8>]) {
        // SAFETY: self.pos is always <= self.storage.len()
        let (init, uninit) = unsafe { self.storage.split_at_unchecked(self.pos) };

        // SAFETY: we have initialized the first self.pos bytes in self.storage
        (unsafe { init.assume_init_ref() }, uninit)
    }

    /// Return mutable references to the initialized and uninitialized portions of our buffer
    pub fn split_mut(&mut self) -> (&mut [u8], &mut [core::mem::MaybeUninit<u8>]) {
        // Safety arguments are the same as above

        let (init, uninit) = unsafe { self.storage.split_at_mut_unchecked(self.pos) };

        (unsafe { init.assume_init_mut() }, uninit)
    }
}

impl<'a> std::io::Write for UninitCursor<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {

        let remaining = self.storage.len().strict_sub(self.pos);
        let to_write = remaining.min(buf.len());

        let (_init, uninit) = self.split_mut();

        uninit[..to_write].write_copy_of_slice(&buf[..to_write]);

        self.pos = self.pos.strict_add(to_write);

        // Note that this does the intended behavior of returning Ok(0)
        // when buf is empty OR when we are full
        Ok(to_write)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// An implementation of [`std::io::Write`] that allows chaining two writers
/// (in our case, in-memory slice cursors) as if they were contiguous
pub struct ChainedWriter<W1, W2> {
    first: W1,
    second: W2,
    using_first: bool,
}

impl<W1: std::io::Write, W2: std::io::Write> std::io::Write for ChainedWriter<W1, W2> {
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
                // this error is non-fatal, and means that this one's done
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

        // Note that this does the intended behavior of returning Ok(0)
        // when buf is empty OR when we are full (i.e. both writers are full)
        Ok(total)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.using_first {
            self.first.flush()?;
        }
        self.second.flush()
    }
}