//! All protocol message types exchanged between endpoints.

use serde::{Deserialize, Serialize};

// Similarly to what the rust compiler does when optimizing data type layouts. you'd probably want
// to create new, flat enums (and deriving serde for them) that will be what gets (en/de)coded over
// the wire. This structural nesting of enums make the bytes occupied by discriminants,
// unnecessarily large, meaning that your throughput gets imapcted, esp. for audio messages.

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
pub enum Error<Fail = (), Refuse = ()> {
    /// The operation failed, but may succeed if retried.
    Failure(Fail),
    /// The operation is permanently unsupported or refused and should not be retried.
    Refusal(Refuse),
}

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
    pub enum Connected {
        /// Control-related messages.
        Control(Control),
        /// Audio data.
        ///
        /// See [`StreamFormats`](crate::format::StreamFormats) for more on stream layout.
        Audio(crate::AudioMessageHeader),
    }
}

/// Messages that can be sent by clients and received by servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Client {
    /// This message, typically sent over a broadcast address, is intended for discovery
    /// by unknown servers in a network.
    Discovery,
    /// Sent in response to a connection request.
    ConnectionResult(Result<(), Error>),
    /// Messages sent after a connection is established.
    Connected(client::Connected),
    /// Sent to indicate that a connection is done,
    Disconnect,
}

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
    pub enum Connected {
        /// Control-related messages.
        Control(Control),
        /// Audio data.
        ///
        /// See [`StreamFormats`](crate::format::StreamFormats) for stream layout.
        Audio(crate::AudioMessageHeader),
    }
}

/// Messages that can be sent by servers and received by clients.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Server {
    /// Requests to connect to a (assumed to be known) client.
    /// Do not send this over broadcast addresses
    Connect(crate::format::StreamFormats),
    /// Messages sent after a connection is established.
    Connected(server::Connected),
    /// Sent to indicate that a connection is done,
    Disconnect,
}
