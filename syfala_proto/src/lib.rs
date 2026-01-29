#![no_std]
//! A simple, low-latency protocol for real-time audio communication and discovery.
//!
//! This crate defines a message-based protocol intended for real-time audio
//! streaming between networked endpoints.
//!
//! ## Roles
//!
//! Each endpoint acts as either a **client** or a **server**:
//!
//! - **Servers** are typically firmware running on external or embedded devices.
//! - **Clients** are typically drivers or applications running on consumer hardware.
//!
//! ## Protocol model
//!
//! The protocol is defined entirely in terms of typed messages exchanged between
//! endpoints. These messages fall into three broad categories:
//!
//! - **Discovery messages**
//! - **Connection messages**
//! - **Control messages**
//! - **Audio messages**
//!
//! See the [`message`] module for the complete message definitions.
//! 
//! ## Discovery
//! 
//! Discovery messages are sent by clients over broadcast addresses to make themselves visible
//! to severs on the network, senrvers may then respond, if they wish, with a connection message
//! to request to establish a connection.
//!
//! ## Connection
//!
//! Connection messages are used to establish communication between endpoints. A server sends
//! a connection message to a client, (typically after receiving a discovery message from it)
//! then the client may accept or refuse.
//!
//! Once this exchange succeeds, a logical "connection" is established.
//!
//! Servers advertise their supported stream formats as part of the connection
//! message. These formats are **fixed for the lifetime of the connection** and
//! define the audio formats used during active I/O.
//!
//! If a client is incompatible with any advertised stream format, it must refuse
//! the connection.
//!
//! ## Control messages
//!
//! Control messages are infrequent messages used to coordinate behavior between
//! connected endpoints. Currently, they are limited to requests to start or stop
//! audio I/O.
//!
//! Clients may request that audio I/O be started. Upon receiving such a request, servers
//! must perform any required initialization, allocation, and clock anchoring **before**
//! replying with a success response.
//!
//! A successful response indicates that the server is _immediately_ ready to send and
//! receive audio data.
//!
//! If the server fails to start I/O, or explicitly refuses the request, it must report
//! the failure back to the client.
//!
//! The same thing happens with Stopping IO, servers free the corresponding resources,
//! then report back.
//!
//! ## Audio messages
//!
//! When a connection is established and I/O is active, endpoints exchange audio
//! messages.
//!
//! Audio messages carry raw audio bytes along with stream indices and byte offsets
//! to allow receivers to interpet how to decode the data and handle packet loss and
//! reordering.
//!
//! The types in this crate already implement `serde`'s `Serialize` and `Deserialize`
//! traits, for the user to conveniently plug into other `serde` backends.

extern crate alloc;

pub mod format;
pub mod message;
pub use serde;

use serde::{Deserialize, Serialize};

/// The header of an audio message.
///
/// The payload of the corresponding audio message contains
/// [`n_bytes`](Self::n_bytes) bytes of, packed, interleaved, uncompressed audio data.
///
/// The [`byte_index`](Self::byte_index) is expressed in **bytes**, not frames,
/// or samples. It is the receiver’s responsibility to interpret this offset
/// according to the negotiated stream format.
///
/// [`n_bytes`](Self::n_bytes) may be **any value**, _including zero_.
/// It may contain partial frames, partial samples, _or data that does not contain
/// any frame or sample boundary at all_
///
/// ## Packet loss and reordering
///
/// - If [`byte_index`](Self::byte_index) is greater than the previous message's
///   [`next_byte_index`](Self::next_byte_idx), then data between the last complete sample and
///   the next complete sample is considered lost.
/// - If [`byte_index`](Self::byte_index) is less than expected (packet reordering),
///   the entire message should be discarded.
///
/// Typical strategies for message loss recovery include silence insertion or more
/// advanced concealment techniques.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AudioStreamMessageHeader {
    pub byte_idx: u64,
    pub n_bytes: u32,
}

impl AudioStreamMessageHeader {
    #[inline(always)]
    pub const fn next_byte_idx(&self) -> u64 {
        self.byte_idx.strict_add(self.n_bytes as u64)
    }
}

/// Audio message header tagged with the index of the stream it belongs to.
///
/// Stream indices are interpreted differently depending on the sender:
///
/// - **Clients** specify the index of the server’s **output** stream. Servers must
/// associate it, in incoming audio messages, with one of their **output** streams.
/// - **Servers** specify the index of on of their **input** streams. Clients must
/// associate it, in incoming audio messages, with one the server's **output** streams
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AudioMessageHeader {
    pub stream_idx: u32,
    pub stream_msg: AudioStreamMessageHeader,
}
