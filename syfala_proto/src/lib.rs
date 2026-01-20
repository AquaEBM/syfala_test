#![no_std]
//! Implementation of a simple protocol for real-time audio communication and discovery
//! 
//! The main idea is the following: A node in the network
//! is a client, a server, or both
//! 
//! A server exposes a device with a fixed set of input/outputs streams, and can send/receive
//! audio as requested by clients connected to it.
//! 
//! Typically, a server is the firmware on a external hardware devices.
//! 
//! A client requests connections to servers and receives their IO stream formats. Clients decide
//! when audio IO should begin/end, by requesting it to servers, and awaiting their response to it
//! 
//! Typically, a client is the driver on consumer devices.

extern crate alloc;

pub mod format;
pub mod message;
pub use postcard;

use serde::{Serialize, Deserialize};

/// Represents a chunk of raw audio data.
///
/// Note that [`byte_index`](Self::byte_index) is in _bytes_. It is up to you
/// to decode it properly according to the format agreed upon with the peer.
///
/// By extension, [`bytes`](Self::bytes) might have any length, including zero.
/// This means that it might contain incomplete frames or samples, or might not contain, __even
/// a frame or sample boundary at all__.
///
/// If [`byte_index`](Self::byte_index) is greater than the `index + bytes.len()` of the
/// previous message (i.e. a packet was lost), then _all bytes from the last frame boundary to
/// the next frame boundary are considered lost_. Typically you would want to replace them with
/// silence, or use a more elaborate scheme to reduce artifacts.
///
/// if it is less (packet reordering), _the entire packet is considered void_.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AudioStreamData<'a> {
    pub byte_index: u64,
    #[serde(borrow)]
    pub bytes: &'a [u8],
}

/// Like [`AudioData`], but includes the index of the stream to use, as well
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AudioData<'a> {
    pub stream_idx: usize,
    #[serde(borrow)]
    pub data: AudioStreamData<'a>,
}
