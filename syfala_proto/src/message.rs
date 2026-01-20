use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum IOState<T = (), U = ()> {
    Start(T),
    Stop(U),
}

/// A generic error message
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Error {
    Failure,
    /// The difference between this variant and failure is the implied meaning that the operation
    /// will never succeed and the requester shouldn't bother retrying.
    Refusal,
}

// Similarly to what the rust compiler does when optimizing data type layouts. you'd probably want
// to create new, flat enums (and deriving serde for them) that will be what gets (en/de)coded over
// the wire. This is because "sub-enum" discriminants are encoded individually, making the
// final, effective, serialized size a bit larger, meaning that your throughput takes a hit
// esp. for audio messages

/// Client-sent messages
pub mod client {
    use super::*;

    /// Control Messages sent from a client to a connected server
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub enum Control {
        /// Sent when requesting to change the state of the IO between us and a server
        /// 
        /// It is important to know that when starting IO, the client must expect audio data
        /// with all _all_ the server's input stream formats, and the server must expect audio
        /// data from with all of it's input stream formats.
        /// 
        /// In short, starting IO (and succeedig) -> starting _all_ __input and output__ streams.
        RequestIOStateChange(IOState),
    }

    // All messages sent from a client to a connected server.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub enum Connected<'a> {
        /// Control messages
        Control(Control),
        /// Audio data
        /// 
        /// See [`StreamFormats`](crate::format::StreamFormats) for more.
        Audio(#[serde(borrow)] crate::AudioData<'a>),
    }
}

/// All message types that can be sent by clients, and received by servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Client<'a> {
    /// Send this message to connect to a new server.
    /// 
    /// Must be sent as a response to a [`Server::Connect`] message,
    /// if the request has been accepted.
    /// 
    /// This mesesage can be used as a discovery beacon.
    /// 
    /// It is not necessary to use it if using, e.g. TCP.
    Connect,
    /// Sent in response to a [`Server::Connect`] message, if the request fails or is refused.
    /// 
    /// It is not necessary to use it if using, e.g. TCP.
    ConnectionError(Error),
    /// All messages sent from a client to an already connected server
    Connected(#[serde(borrow)] client::Connected<'a>),
}

/// Server-sent messages
pub mod server {
    use super::*;

    /// Control Messages sent from a client to a connected server
    #[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
    pub enum Control {
        /// Sent in response to a [`client::Control::RequestIOStateChange`] message
        IOStateChangeResult(IOState<Result<(), Error>, Result<(), Error>>),
    }

    // All messages sent from a server to a connected client.
    #[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
    pub enum Connected<'a> {
        /// Control Messages
        Control(Control),
        /// Audio data.
        /// 
        /// See [`StreamFormats`](crate::format::StreamFormats) for more.
        Audio(#[serde(borrow)] crate::AudioData<'a>),
    }
}

/// All message types that can be sent by servers, and received by clients.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Server<'a> {
    /// Send this message to connect to a new client.
    /// 
    /// Must be sent as a response to a [`Client::Connect`] message,
    /// if the request has been accepted.
    /// 
    /// This mesesage can be used as a discovery beacon.
    /// 
    /// It is not necessary to use it if using, e.g. TCP.
    Connect(crate::format::StreamFormats),
    /// Sent in response to a [`Client::Connect`] message, if the request fails or is refused.
    /// 
    /// It is not necessary to use it if using, e.g. TCP.
    ConnectionError(Error),
    /// All messages sent from a server to an already connected client
    Connected(#[serde(borrow)] server::Connected<'a>),
}