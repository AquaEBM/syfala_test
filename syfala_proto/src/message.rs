//! All protocol message types exchanged between endpoints.

use serde::{Deserialize, Serialize};

// Similarly to what the rust compiler does when optimizing data type layouts. you'd probably want
// to create new, flat enums that will be what gets (en/de)coded over
// the wire. This structural nesting of enums make the bytes occupied by discriminants,
// unnecessarily large, which can impact throughput, esp. for audio messages.

/// Represents a requested or resulting audio IO state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IOState<T = (), U = ()> {
    /// Request or indicate that audio IO should start.
    Start(T),
    /// Request or indicate that audio IO should stop.
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
        /// Requests a change in the audio IO state.
        ///
        /// # Note
        ///
        /// When starting IO, both sides must expect **all** advertised input
        /// and output streams to become active _simultaneously_, and for as long as IO is active.
        RequestIOStateChange(IOState),
        /// Sent periodically to notify the server that our connection to them is still active.
        Heartbeat,
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
    /// Sent to indicate that a connection has been terminated.
    Disconnect,
}

impl Client {
    pub const HEARTBEAT: Self =
        Self::Connected(client::Connected::Control(client::Control::Heartbeat));
    pub const START_IO: Self = Self::Connected(client::Connected::Control(
        client::Control::RequestIOStateChange(IOState::Start(())),
    ));
    pub const STOP_IO: Self = Self::Connected(client::Connected::Control(
        client::Control::RequestIOStateChange(IOState::Stop(())),
    ));
    pub const CONN_SUCCESS: Self = Self::ConnectionResult(Ok(()));
    pub const CONN_FAILED: Self = Self::ConnectionResult(Err(Error::Failure(())));
    pub const CONN_REFUSED: Self = Self::ConnectionResult(Err(Error::Refusal(())));

    pub const fn audio(header: crate::AudioMessageHeader) -> Self {
        Self::Connected(client::Connected::Audio(header))
    }
}

pub mod server {
    use super::*;

    /// Control messages sent from a server to a connected client.
    #[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
    pub enum Control {
        /// Response to a client IO state change request.
        IOStateChangeResult(IOState<Result<(), Error>, Result<(), Error>>),
        /// Sent periodically to notify the server that our connection to them is still active.
        Heartbeat,
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
    ///
    /// Do not send this over broadcast addresses.
    Connect(crate::format::StreamFormats),
    /// Messages sent after a connection is established.
    Connected(server::Connected),
    /// Sent to indicate that a connection has been terminated.
    Disconnect,
}

impl Server {
    pub const HEARTBEAT: Self =
        Self::Connected(server::Connected::Control(server::Control::Heartbeat));
    pub const START_IO_OK: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Start(Ok(()))),
    ));
    pub const START_IO_FAILED: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Start(Err(Error::Failure(())))),
    ));
    pub const START_IO_REFUSED: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Start(Err(Error::Refusal(())))),
    ));
    pub const STOP_IO_OK: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Stop(Ok(()))),
    ));
    pub const STOP_IO_FAILED: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Stop(Err(Error::Failure(())))),
    ));
    pub const STOP_IO_REFUSED: Self = Self::Connected(server::Connected::Control(
        server::Control::IOStateChangeResult(IOState::Stop(Err(Error::Refusal(())))),
    ));

    pub const fn audio(header: crate::AudioMessageHeader) -> Self {
        Server::Connected(server::Connected::Audio(header))
    }
}
