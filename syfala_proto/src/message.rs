//! All protocol message types exchanged between endpoints.

use serde::{Deserialize, Serialize};

// Similarly to what the rust compiler does when optimizing data type layouts. you'd probably want
// to create new, flat enums (and deriving serde for them) that will be what gets (en/de)coded over
// the wire. This is because "sub-enum" discriminants are encoded individually, making the
// final, effective, serialized size a bit larger, meaning that your throughput gets imapcted
// esp. for audio messages.

/// Represents a requested or resulting audio I/O state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IOState<T = (), U = ()> {
    /// Request or indicate that audio I/O should start.
    Start(T),
    /// Request or indicate that audio I/O should stop.
    Stop(U),
}

/// A generic error message used during connection or control handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Error {
    /// The operation failed, but may succeed if retried.
    Failure,
    /// The operation is permanently unsupported or refused and should not be retried.
    Refusal,
}

/// Messages sent by clients to connected servers.
pub mod client {
    use super::*;

    /// Control messages sent from a client to a connected server.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub enum Control {
        /// Requests a change in the audio I/O state.
        ///
        /// # Note
        /// 
        /// When starting I/O, both sides must expect **all** advertised input
        /// and output streams to become active _simultaneously_, and for as long as IO is active.
        RequestIOStateChange(IOState),
    }

    /// Messages sent by a client after a connection is established.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub enum Connected<'a> {
        /// Control-related messages.
        Control(Control),
        /// Audio data.
        ///
        /// See [`StreamFormats`](crate::format::StreamFormats) for more on stream layout.
        Audio(#[serde(borrow)] crate::AudioData<'a>),
    }
}

/// Messages that can be sent by clients and received by servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Client<'a> {
    /// Requests a new connection.
    ///
    /// May also be used as a discovery beacon.
    Connect,
    /// Sent if a connection attempt fails or is refused.
    /// 
    /// Typically, a client should send this if it doesn't support any of a server's
    /// advertised stream formats.
    ConnectionError(Error),
    /// Messages sent after a connection is established.
    Connected(#[serde(borrow)] client::Connected<'a>),
}

/// Messages sent by servers to connected clients.
pub mod server {
    use super::*;

    /// Control messages sent from a server to a connected client.
    #[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
    pub enum Control {
        /// Response to a client I/O state change request.
        IOStateChangeResult(IOState<Result<(), Error>, Result<(), Error>>),
    }

    /// Messages sent by a server after a connection is established.
    #[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
    pub enum Connected<'a> {
        /// Control-related messages.
        Control(Control),
        /// Audio data.
        ///
        /// See [`StreamFormats`](crate::format::StreamFormats) for stream layout.
        Audio(#[serde(borrow)] crate::AudioData<'a>),
    }
}

/// Messages that can be sent by servers and received by clients.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Server<'a> {
    /// Accepts a connection and advertises supported stream formats.
    ///
    /// May also be used as a discovery beacon.
    Connect(crate::format::StreamFormats),
    /// Sent if a connection attempt fails or is refused.
    ConnectionError(Error),
    /// Messages sent after a connection is established.
    Connected(#[serde(borrow)] server::Connected<'a>),
}