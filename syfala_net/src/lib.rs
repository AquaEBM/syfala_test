use core::{iter, num};
use std::{io, thread};

pub mod network;
pub mod queue;
mod timing;

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
    pub const fn chunk_size_samples(&self) -> num::NonZeroU32 {
        self.chunk_size_frames()
            .checked_mul(self.n_channels())
            .unwrap()
    }
}

#[derive(Clone)]
pub struct Waker {
    thread_handle: thread::Thread,
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
    fn wake(&self) {
        self.thread_handle.unpark();
    }
}

fn reshape_iter(
    drift: Option<timing::Drift>,
    iterator: impl IntoIterator<Item = Sample>,
) -> impl Iterator<Item = Sample> {
    // Notice how neither are positive at the same time
    let (padding_spls, skipped_spls) = if let Some(drift) = drift {
        if drift.is_negative() {
            (0, drift.abs().get())
        } else {
            (drift.abs().get(), 0)
        }
    } else {
        (0, 0)
    };

    return iter::repeat_n(SILENCE, padding_spls).chain(iterator.into_iter().skip(skipped_spls));
}
