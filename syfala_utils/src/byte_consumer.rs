//! Streaming utilities for converting indexed byte streams back into samples.
//!
//! This module provides stateful adapters to:
//! - consume byte streams that may start at arbitrary byte indices,
//! - reconstruct samples from those bytes,
//! - and insert padding samples when data is missing or misaligned.
//!
//! The API is iterator-based and designed to tolerate partial consumption
//! and packet loss, making it suitable for real-time audio transport.

use crate::{SampleFromBytes, SampleTypeSilence, queue};

use core::{num, iter, marker};
use alloc::boxed::Box;

/// A sink for consuming samples produced by a stream.
///
/// This trait abstracts over entities that *accept* samples, without
/// constraining how those samples are ultimately stored or processed.
pub trait SampleSink {
    /// The sample type accepted by this sink.
    type Sample;

    /// Consume a sequence of samples.
    ///
    /// Implementations are free to partially or fully consume the iterator.
    fn consume_samples(&mut self, spls: impl IntoIterator<Item = Self::Sample>);
}

/// Implementation of [`SampleSink`] for an `rtrb::Producer`.
///
/// All samples yielded by the iterator are written into the producer
/// as long as capacity permits.
impl<T> SampleSink for rtrb::Producer<T> {
    type Sample = T;

    fn consume_samples(&mut self, spls: impl IntoIterator<Item = Self::Sample>) {
        queue::producer_get_all(self).fill_from_iter(spls);
    }
}

// We can't do something like this yet:
//
// pub struct AudioSamplePadder<T: SampleType> {
//     current_byte_idx: u64,
//     current_sample_bytes: [u8 ; T::SIZE],
// }
//
// without NIGHTLY: #[feature(min_generic_const_args)]
// So, yes, the following feels a bit hacky

/// Stateful adapter that reconstructs samples from indexed byte streams.
///
/// The padder tracks the global byte index and inserts padding samples
/// whenever bytes are missing or misaligned with respect to sample boundaries.
#[derive(Debug)]
pub struct AudioPacketSamplePadder<T: SampleFromBytes> { // name bikeshedding welcome
    /// Current global byte index expected by the stream.
    current_byte_idx: u64,
    /// Buffer holding the bytes of the partially reconstructed sample.
    ///
    /// Invariant: its length is always equal to `T::SIZE`.
    current_sample_bytes: Box<[u8]>,
    /// Marker tying the padder to its sample type.
    _marker: marker::PhantomData<T>,
}

impl<T: SampleFromBytes> Default for AudioPacketSamplePadder<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SampleFromBytes> AudioPacketSamplePadder<T> {
    /// Create a new `AudioPacketSamplePadder`.
    ///
    /// The padder starts at byte index `0` with an empty sample buffer.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            current_byte_idx: 0,
            current_sample_bytes: iter::repeat_n(0, usize::from(T::SIZE.get())).collect(),
            _marker: marker::PhantomData,
        }
    }

    /// Feed a packet of bytes into the padder and obtain reconstructed samples.
    ///
    /// The provided `byte_idx` indicates the starting position of the byte
    /// iterator in the global byte stream. If bytes are missing relative to
    /// the expected index, padding samples are generated using `pad_fn`.
    ///
    /// Bytes that belong to incomplete samples are buffered internally until
    /// enough data is available to reconstruct a full sample.
    #[inline(always)]
    pub fn feed_bytes(
        &mut self,
        byte_idx: u64,
        bytes: impl IntoIterator<Item = u8>,
        pad_fn: impl FnMut() -> T,
    ) -> impl IntoIterator<Item = T> {
        let sample_size = num::NonZeroUsize::from(T::SIZE).get();
        assert_eq!(sample_size, self.current_sample_bytes.len());

        let (n_padding_spls, n_skipped_bytes) = match byte_idx.cmp(&self.current_byte_idx) {
            // reordered packet, skip all bytes
            core::cmp::Ordering::Less => (0usize, usize::MAX),
            // correct packet index, don't pad or skip
            core::cmp::Ordering::Equal => (0, 0),
            core::cmp::Ordering::Greater => {
                let bps = num::NonZeroU64::from(T::SIZE);

                // previous valid sample index
                let prev_spl_idx = self.current_byte_idx / bps;
                // next valid sample index
                let next_spl_idx = byte_idx.strict_add(bps.get().strict_sub(1)) / bps;

                let n_padding_samples = next_spl_idx.strict_sub(prev_spl_idx);

                let next_spl_byte_idx = next_spl_idx.strict_mul(bps.get());

                let n_skipped_bytes = next_spl_byte_idx.strict_sub(self.current_byte_idx);
                self.current_byte_idx = next_spl_byte_idx;

                (
                    n_padding_samples.try_into().unwrap(),
                    n_skipped_bytes.try_into().unwrap(),
                )
            }
        };

        // insert padding (pad_fn) in place of incomplete samples
        let padding_iter = iter::repeat_with(pad_fn).take(n_padding_spls);

        // also a bit hacky
        // i don't see any way to make this cleaner
        // without using NIGHTLY: #[feature(iter_array_chunks)]
        let sample_iter = bytes
            .into_iter()
            .skip(n_skipped_bytes)
            .filter_map(move |byte| {
                let curr = usize::try_from(self.current_byte_idx % num::NonZeroU64::from(T::SIZE))
                    .unwrap();

                self.current_sample_bytes[curr] = byte;
                self.current_byte_idx = self.current_byte_idx.strict_add(1);

                if self.current_byte_idx % num::NonZeroU64::from(T::SIZE) != 0 {
                    return None;
                }

                let this_sample_bytes =
                    &self.current_sample_bytes[curr.strict_sub(sample_size)..curr];

                Some(T::from_bytes(this_sample_bytes))
            });

        iter::chain(padding_iter, sample_iter)
    }
}

// We can also do something like this for frames, if you wish to discard
// whole frames on byte loss, (as is recommended in the documentation
// for AudioMessageHeader)
// 
// pub struct AudioPacketFramePadder<T: SampleType> {
//     current_byte_idx: u64,
//     current_frame_bytes: Box<[[u8 ; T::SIZE]]>,
// }
// 
// but we just keep it simple with samples for now.

/// Framing abstraction that converts indexed byte streams into samples.
///
/// Implementations handle byte alignment and may insert padding samples
/// to compensate for missing data.
pub trait ByteStreamFramer {
    /// The sample type produced by the framer.
    type Sample;

    /// Frame a sequence of bytes into samples.
    ///
    /// The `byte_idx` parameter indicates the starting position of the
    /// byte iterator in the global stream.
    fn frame_bytes(
        &mut self,
        byte_idx: u64,
        bytes: impl IntoIterator<Item = u8>,
    ) -> impl IntoIterator<Item = Self::Sample>;
}

/// [`ByteStreamFramer`] implementation for [`AudioPacketSamplePadder`].
/// 
/// Missing or incomplete samples are padded using the sample type's
/// silence value.
impl<T: SampleFromBytes + SampleTypeSilence> ByteStreamFramer for AudioPacketSamplePadder<T> {
    type Sample = T;

    fn frame_bytes(
        &mut self,
        byte_idx: u64,
        bytes: impl IntoIterator<Item = u8>,
    ) -> impl IntoIterator<Item = Self::Sample> {
        self.feed_bytes(byte_idx, bytes, || T::SILENCE)
    }
}

/// Adapter combining a byte stream framer and a sample sink.
/// 
/// Incoming byte packets are framed into samples and immediately
/// forwarded to the sink.
pub struct IndexedAudioByteStreamSender<S, F> {
    /// Sink that consumes reconstructed samples.
    sink: S,
    /// Framer responsible for turning bytes into samples.
    framer: F,
}

/// Consumer of indexed audio packets.
/// 
/// Each packet consists of a starting byte index and an iterator of bytes.
pub trait AudioPacketConsumer {
    /// Consume a packet of bytes starting at the given byte index.
    ///
    /// Implementations may choose to only partially consume the iterator.
    fn consume_packet(&mut self, byte_idx: u64, bytes: impl IntoIterator<Item = u8>);
}

impl<S: SampleSink, F: ByteStreamFramer<Sample = S::Sample>> AudioPacketConsumer
    for IndexedAudioByteStreamSender<S, F>
{
    /// Consume a packet by framing its bytes into samples and forwarding
    /// them to the underlying sink.
    fn consume_packet(&mut self, byte_idx: u64, bytes: impl IntoIterator<Item = u8>) {
        self.sink
            .consume_samples(self.framer.frame_bytes(byte_idx, bytes));
    }
}
