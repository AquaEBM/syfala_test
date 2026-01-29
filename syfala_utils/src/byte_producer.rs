//! Streaming utilities for converting sample iterators into byte iterators.
//! 
//! This module provides small, composable building blocks to:
//! - pull samples from a source,
//! - frame them into a continuous byte stream,
//! - and produce indexed byte packets suitable for transport or buffering.
//! 
//! The design is iterator-based, stateful, and allocation-free (Well, in the hot path),
//! making it suitable for real-time use cases.

use crate::{SampleToBytes, SampleSize, queue};

use core::{num, iter, marker};
use alloc::boxed::Box;

/// A source of samples that can be polled to obtain an iterator of samples.
/// 
/// This trait abstracts over entities that *produce* samples, without
/// prescribing how those samples are stored or generated.
pub trait SampleSource {
    /// The sample type produced by this source.
    type Sample;

    /// Retrieve an iterator over available samples.
    ///
    /// The returned iterator may be empty if no samples are currently available.
    fn get_samples(&mut self) -> impl IntoIterator<Item = Self::Sample>;
}

/// Implementation of [`SampleSource`] for an `rtrb::Consumer`.
///
/// All currently available samples are drained from the consumer.
impl<T> SampleSource for rtrb::Consumer<T> {
    type Sample = T;

    fn get_samples(&mut self) -> impl IntoIterator<Item = Self::Sample> {
        queue::consumer_get_all(self)
    }
}

// We need to make a custom iterator (instead of closures + flatmap)
// or the borrow checker will complain

/// Iterator that yields a byte stream from an iterator of samples.
///
/// This iterator keeps track of the current byte index globally and
/// converts samples to bytes lazily, only when a new sample boundary
/// is reached.
struct SampleByteStreamIter<'a, I> {
    /// Iterator yielding samples to be converted.
    iter: I,
    /// Global byte index into the logical byte stream.
    current_byte_idx: &'a mut u64,
    /// Scratch buffer holding the bytes of the current sample.
    current_sample_bytes: &'a mut [u8],
}

impl<'a, I: Iterator<Item: SampleToBytes>> Iterator for SampleByteStreamIter<'a, I> {
    type Item = u8;

    /// Yield the next byte in the stream.
    ///
    /// When a sample boundary is crossed, the next sample is fetched
    /// and converted into bytes before continuing.
    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let current_spl_byte_idx = *self.current_byte_idx % num::NonZeroU64::from(I::Item::SIZE);

        if current_spl_byte_idx == 0 {
            self.iter.next()?.to_bytes(self.current_sample_bytes);
        }

        *self.current_byte_idx = self.current_byte_idx.strict_add(1);

        Some(self.current_sample_bytes[usize::try_from(current_spl_byte_idx).unwrap()])
    }

    // TODO: implement nth and size_hint
}

// the same setback mentioned in byte_consumer occurs here
// NIGHTLY: #[feature(min_generic_const_args)]

/// Stateful adapter that converts streams of samples into streams of bytes.
///
/// The adapter preserves byte alignment across successive calls, ensuring
/// that partially-consumed samples are resumed correctly when more samples
/// are fed.
pub struct SampleByteStream<T: SampleToBytes> {
    /// Buffer holding the bytes of the currently active sample.
    current_sample_bytes: Box<[u8]>,
    /// Global byte index into the logical byte stream.
    current_byte_idx: u64,
    _marker: marker::PhantomData<T>,
}

impl<T: SampleToBytes> SampleByteStream<T> {
    /// Create a new `SampleByteStream`.
    ///
    /// The stream starts at byte index `0` and with an empty sample buffer.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            current_sample_bytes: iter::repeat_n(0, usize::from(T::SIZE.get())).collect(),
            current_byte_idx: 0,
            _marker: marker::PhantomData,
        }
    }

    /// Return the current global byte index.
    ///
    /// This corresponds to the number of bytes that have been logically
    /// produced so far by the stream.
    #[inline(always)]
    pub fn current_byte_idx(&self) -> u64 {
        self.current_byte_idx
    }

    /// Feed a sequence of samples into the stream and obtain an iterator of bytes.
    ///
    /// The returned iterator may be partially consumed; any remaining bytes
    /// are preserved internally and will be yielded first on the next call.
    #[inline(always)]
    pub fn feed_samples(
        &mut self,
        samples: impl IntoIterator<Item = T>,
    ) -> impl IntoIterator<Item = u8> {
        SampleByteStreamIter {
            iter: samples.into_iter(),
            current_byte_idx: &mut self.current_byte_idx,
            current_sample_bytes: self.current_sample_bytes.as_mut(),
        }
    }
}

/// Framing abstraction that turns samples into indexed byte streams.
///
/// Implementations return both the starting byte index and an iterator
/// yielding the framed bytes.
pub trait SampleStreamFramer {
    /// The sample type being framed.
    type Sample;

    /// Frame a sequence of samples into a byte iterator.
    /// 
    /// The returned index represents the byte position at which the
    /// iterator starts.
    fn frame_samples(
        &mut self,
        samples: impl IntoIterator<Item = Self::Sample>,
    ) -> (u64, impl IntoIterator<Item = u8>);
}

/// [`SampleStreamFramer`] implementation for [`SampleByteStream`].
/// 
/// Framing corresponds to exposing the current byte index and delegating
/// to `feed_samples`.
impl<T: SampleToBytes> SampleStreamFramer for SampleByteStream<T> {
    type Sample = T;

    fn frame_samples(
        &mut self,
        samples: impl IntoIterator<Item = Self::Sample>,
    ) -> (u64, impl IntoIterator<Item = u8>) {
        (self.current_byte_idx(), self.feed_samples(samples))
    }
}

/// Producer of indexed audio packets.
///
/// Each call yields a starting byte index along with an iterator of bytes
/// representing the packet payload.
pub trait AudioPacketProducer {

    /// Produce the next audio packet.
    ///
    /// The returned iterator may be only partially consumed by the caller;
    /// implementations should ensure that unconsumed bytes are not dropped.
    fn produce_packet(&mut self) -> (u64, impl IntoIterator<Item = u8>);
}

/// Adapter combining a sample source and a stream framer.
///
/// Samples are pulled from the source and immediately framed into
/// indexed byte packets.
pub struct IndexedAudioSampleStreamReceiver<S, F> {
    /// Underlying sample source.
    source: S,
    /// Framer used to convert samples into bytes.
    framer: F,
}

impl<S: SampleSource, F: SampleStreamFramer<Sample = S::Sample>> AudioPacketProducer
    for IndexedAudioSampleStreamReceiver<S, F>
{
    /// Produce a packet by pulling all available samples from the source
    /// and framing them into a byte stream.
    fn produce_packet(&mut self) -> (u64, impl IntoIterator<Item = u8>) {
        self.framer.frame_samples(self.source.get_samples())
    }
}