//! Implementation of a simple protocol for real-time audio communication and discovery

use core::num;

pub mod network;

#[cfg(feature = "rtrb")]
pub mod queue;
mod timing;

#[inline(always)]
const fn nz(x: usize) -> num::NonZeroUsize {
    num::NonZeroUsize::new(x).unwrap()
}

pub type Sample = f32;
pub const SILENCE: Sample = 0.;

pub const SAMPLE_SIZE: num::NonZeroUsize = nz(size_of::<Sample>());

/// Represents a server's audio configuration. May have more fields in the future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioConfig {
    n_channels: num::NonZeroU32,
    buffer_size_frames: num::NonZeroU32,
}

impl AudioConfig {
    pub const fn new(n_channels: num::NonZeroU32, buffer_size_frames: num::NonZeroU32) -> Self {
        Self {
            n_channels,
            buffer_size_frames,
        }
    }

    #[inline(always)]
    pub const fn n_channels(&self) -> num::NonZeroU32 {
        self.n_channels
    }

    #[inline(always)]
    pub const fn chunk_size_frames(&self) -> num::NonZeroU32 {
        self.buffer_size_frames
    }

    /// This is the same as [`self.n_channels()`](Self::n_channels)` *
    /// `[`self.chunk_size_frames()`](Self::chunk_size_frames)
    #[inline(always)]
    pub fn chunk_size_samples(&self) -> num::NonZeroU32 {
        self.chunk_size_frames()
            .checked_mul(self.n_channels())
            .unwrap()
    }
}

/// Enables waking a thread in a periodic manner, usually used in conjunction
/// with the queues in [`queue`]. Use [`Waker::useless`] for a waker that does nothing.
#[derive(Debug, Clone)]
pub struct Waker {
    thread_handle: std::thread::Thread,
    chunk_size_spls: num::NonZeroUsize,
}

impl Default for Waker {
    fn default() -> Self {
        Self::useless()
    }
}

impl Waker {
    #[inline(always)]
    pub fn useless() -> Self {
        Self::new(std::thread::current(), num::NonZeroUsize::MAX)
    }

    #[inline(always)]
    pub const fn new(
        thread_handle: std::thread::Thread,
        chunk_size_spls: num::NonZeroUsize,
    ) -> Self {
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
    fn wake(&self) {
        self.thread_handle.unpark();
    }
}
