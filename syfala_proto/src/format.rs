//! Audio stream format definitions.

use alloc::boxed::Box;
use core::{fmt, num};
use serde::{Deserialize, Serialize};

/// Supported sample formats.
///
/// All samples are assumed to be packed (no unused bytes), little-endian, interleaved, and
/// uncompressed
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum SampleType {
    U8,
    U16,
    U24,
    U32,
    U64,
    I8,
    I16,
    I24,
    I32,
    I64,
    IEEF32,
    IEEF64,
}

impl SampleType {
    /// Returns whether the format is signed (including floating-point types).
    #[inline(always)]
    pub const fn is_signed(self) -> bool {
        use SampleType::*;
        matches!(self, I8 | I16 | I24 | I32 | IEEF32 | IEEF64)
    }

    /// Returns whether the format is floating-point.
    #[inline(always)]
    pub fn is_float(self) -> bool {
        use SampleType::*;
        matches!(self, IEEF32 | IEEF64)
    }

    /// Returns the size of a single sample in bytes.
    #[inline(always)]
    pub const fn sample_size(self) -> num::NonZeroU8 {
        use SampleType::*;
        let res = match self {
            U8 | I8 => 1,
            U16 | I16 => 2,
            U24 | I24 => 3,
            U32 | I32 | IEEF32 => 4,
            U64 | I64 | IEEF64 => 8,
        };

        num::NonZeroU8::new(res).unwrap()
    }
}

/// A validated audio sample rate.
///
/// The inner value is guaranteed to be positive and normal.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(try_from = "f64")]
pub struct SampleRate(f64);

impl SampleRate {
    #[inline(always)]
    pub const fn get(&self) -> &f64 {
        &self.0
    }

    /// Creates a new sample rate if the value is positive and
    /// [normal](https://en.wikipedia.org/wiki/Normal_number_(computing)).
    #[inline(always)]
    pub const fn new(val: f64) -> Option<Self> {
        if val.is_normal() && val.is_sign_positive() {
            Some(Self(val))
        } else {
            None
        }
    }
}

/// Error returned when creating an invalid [`SampleRate`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct SampleRateError;

impl fmt::Display for SampleRateError {
    #[inline(always)]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Sample rate must be normal and positive")
    }
}

impl TryFrom<f64> for SampleRate {
    type Error = SampleRateError;

    #[inline(always)]
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(SampleRateError)
    }
}

/// Number of audio channels.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ChannelCount(pub num::NonZeroU32);

/// Buffer size expressed in frames.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct BufferSize(pub u32);

/// A complete audio stream format description.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Format {
    pub sample_rate: SampleRate,
    pub channel_count: ChannelCount,
    /// Buffer size hint, expressed in frames.
    ///
    /// This value is advisory and does not constrain packet sizes.
    /// 
    /// If it is zero, then it must be considered as not provided.
    // TODO: move this value to StartIO messages...
    pub buffer_size: BufferSize,
    pub sample_type: SampleType,
}

impl Default for Format {
    #[inline(always)]
    fn default() -> Self {
        Self::standard()
    }
}

impl Format {
    /// Returns the default format:
    /// 
    /// IEEF32, 48 kHz, stereo, 32-frame buffering.
    #[inline(always)]
    pub const fn standard() -> Format {
        Format {
            sample_rate: SampleRate::new(48e3).unwrap(),
            channel_count: ChannelCount(num::NonZeroU32::new(2).unwrap()),
            buffer_size: BufferSize(32),
            sample_type: SampleType::IEEF32,
        }
    }

    /// Returns the number of samples per buffer, if a buffer size is specified.
    #[inline(always)]
    pub fn chunk_size_samples(&self) -> Option<num::NonZeroU32> {
        num::NonZeroU32::new(self.buffer_size.0)
            .map(|n| n.checked_mul(self.channel_count.0).unwrap())
    }

    /// Returns the number of bytes per buffer, if a buffer size is specified.
    #[inline(always)]
    pub fn chunk_size_bytes(&self) -> Option<num::NonZeroU32> {
        self.chunk_size_samples().map(|n| {
            n.checked_mul(self.sample_type.sample_size().into())
                .unwrap()
        })
    }
}

/// Describes all input and output stream formats of a server.
///
/// When I/O starts:
/// - Clients must send audio for **all output streams**
/// - Servers must send audio for **all input streams**
///
/// Stream counts and formats are fixed for the lifetime of the connection.
#[derive(Debug, Default, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct StreamFormats {
    pub inputs: Box<[Format]>,
    pub outputs: Box<[Format]>,
}

impl AsRef<StreamFormats> for StreamFormats {
    fn as_ref(&self) -> &StreamFormats {
        self
    }
}
