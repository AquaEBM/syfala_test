use core::{iter, num};
use std::{io, thread};

pub mod client;
pub mod queue;
pub mod server;
pub mod timing;

/// Convenience re-export of rtrb
pub use rtrb;

#[inline(always)]
const fn nz(x: usize) -> num::NonZeroUsize {
    num::NonZeroUsize::new(x).unwrap()
}

pub type Sample = f32;
pub const SILENCE: Sample = 0.;

pub const SAMPLE_SIZE: num::NonZeroUsize = nz(size_of::<Sample>());

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AudioConfig {
    n_channels: num::NonZeroUsize,
    buffer_size_frames: num::NonZeroUsize,
}

impl AudioConfig {
    pub const fn new(n_channels: num::NonZeroUsize, buffer_size_frames: num::NonZeroUsize) -> Self {
        Self {
            n_channels,
            buffer_size_frames,
        }
    }

    #[inline(always)]
    pub const fn n_channels(&self) -> num::NonZeroUsize {
        self.n_channels
    }

    #[inline(always)]
    pub const fn chunk_size_frames(&self) -> num::NonZeroUsize {
        self.buffer_size_frames
    }

    /// This is the same as [`self.n_channels()`](Self::n_channels)` *
    /// `[`self.chunk_size_frames()`](Self::chunk_size_frames)
    #[inline(always)]
    pub const fn chunk_size_samples(&self) -> num::NonZeroUsize {
        self.chunk_size_frames()
            .checked_mul(self.n_channels())
            .unwrap()
    }
}
