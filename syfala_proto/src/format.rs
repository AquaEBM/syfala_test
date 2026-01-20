use alloc::boxed::Box;
use core::{fmt, num};
use serde::{Deserialize, Serialize};

/// All possible sample format types supported by our protocol.
/// 
/// Note that litte-endian, interleaved, uncompressed, is assumed.
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

    /// Returns whether this is a signed format (includes floats)
    #[inline(always)]
    pub const fn is_signed(self) -> bool {
        use SampleType::*;
        matches!(self, I8 | I16 | I24 | I32 | IEEF32 | IEEF64)
    }

    /// Returns whether this is a floating point format
    #[inline(always)]
    pub fn is_float(self) -> bool {
        use SampleType::*;
        matches!(self, IEEF32 | IEEF64)
    }

    /// Returns the number of bytes occupied by a sample in this format
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

/// A newtype wrapper around a sample rate, as an `f64`
///
/// The inner value is always positive and normal.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(try_from = "f64")]
pub struct SampleRate(f64);

impl SampleRate {
    #[inline(always)]
    pub const fn get(&self) -> &f64 {
        &self.0
    }

    /// Returns `Some(val)` if `val.is_normal() && val.is_sign_positive()`
    /// 
    /// Returns `None` otherwise.
    #[inline(always)]
    pub const fn new(val: f64) -> Option<Self> {
        if val.is_normal() && val.is_sign_positive() {
            return Some(Self(val));
        }
        None
    }
}

/// The error type, when creating an invalid sample rate.
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

/// A newtype wapper around an integer representing a channel count
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ChannelCount(pub num::NonZeroU32);

/// A newtype wapper around an integer representing a buffer size.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct BufferSize(pub num::NonZeroU32);

/// Represents an audio stream configuration.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Format {
    pub sample_rate: SampleRate,
    pub channel_count: ChannelCount,
    /// Important: This value is in ___frames___
    ///
    /// Note that this field typically serves as a hint for senders, to help them decide how
    /// fast to send data and is in no way a constraint on packet contents/sizes
    pub buffer_size: Option<BufferSize>,
    pub sample_type: SampleType,
}

impl Default for Format {
    #[inline(always)]
    fn default() -> Self {
        Self::standard()
    }
}

impl Format {
    #[inline(always)]
    /// The default format, IEEF32, 48kHz, 1 ch, 32-frame buffering
    pub const fn standard() -> Format {
        Format {
            sample_rate: SampleRate::new(48e3).unwrap(),
            channel_count: ChannelCount(num::NonZeroU32::new(1).unwrap()),
            buffer_size: Some(BufferSize(num::NonZeroU32::new(32).unwrap())),
            sample_type: SampleType::IEEF32,
        }
    }

    /// This is the same as [`self.channel_count`](Self::channel_count)` *
    /// `[`self.buffer_size`](Self::buffer_size)
    #[inline(always)]
    pub fn chunk_size_samples(&self) -> Option<num::NonZeroU32> {
        self.buffer_size
            .and_then(|n| n.0.checked_mul(self.channel_count.0))
    }

    /// This is the same as [`self.chunk_size_samples`](Self::chunk_size_samples)` * `[`self.sample_type.sample_size()`](SampleType::sample_size)
    #[inline(always)]
    pub fn chunk_size_bytes(&self) -> Option<num::NonZeroU32> {
        self.chunk_size_samples()
            .and_then(|n| n.checked_mul(self.sample_type.sample_size().into()))
    }
}

/// Represents _all_ the stream formats of a server.
/// 
/// __Important:__
/// 
///  - Servers should _send_ audio data to clients in the formats specified by their
/// __[`inputs`](Self::inputs)__.
/// 
///  - Clients should interpret _incoming_ audio data in the formats
/// specified by the server's advertised __[`inputs`](Self::inputs)__.
/// 
///  - Servers should interpret _incoming_ audio data in the formats specified by their
/// __[`outputs`](Self::outputs)__.
/// 
///  - Clients should _send_ audio data to clients in the format's specified by the server's
/// advertised __[`outputs`](Self::outputs)__.
/// 
/// 
/// For example, a server advertising 2 48khz input streams and 3 96khz output streams expects to
/// __receive__ 3 96khz audio streams from each client, and should __send__ 2 48khz audio streams to
/// each client.
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
