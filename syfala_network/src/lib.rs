//! Implementation of the message model defined in the `proto` crate.
//!
//! This crate provides the runtime machinery needed to send and receive protocol
//! messages over network sockets, along with lightweight abstractions for client and
//! server state management.
//!
//! ## Scope
//!
//! - Encoding and decoding protocol messages using [`serde`] and [`postcard`].
//! - Transport over network sockets (currently, UDP only)
//! - Small helper traits for driving client and server states
//!
//! This crate is intentionally transport-focused: it does not redefine the
//! protocol itself, but instead implements a concrete wire representation and
//! communication layer for the message model described in `proto`.

pub mod udp;
pub use postcard;
pub use syfala_proto as proto;

use proto::serde::{Deserialize, Serialize};

// Internal types with flat enum representations so serde and postcard don't waste bandwidth
// serializing/deserializing nested enums.
//
// These enums remove structural nesting present in `proto::message` and instead
// encode each meaningful message variant directly as a single discriminant. This mirrors
// the kind of layout flattening performed by the Rust compiler for in-memory enums, but
// applied explicitly at the wire level.
//
// Callers of this library never see these types. They are used exclusively at the
// serialization boundary in `{Client,Server}::{send,recv}`, and are converted to and from
// the public protocol message types automatically.
//
// See the relevant comment in `proto::message` for more.

// NOTE: We specify discriminants explicitly, but we do not have to, we just do this to make
// sure our packet format is stable, debuggable, and robust (1 byte discriminants are too
// insecure)

/// Flattened wire representation of client-to-server messages.
///
/// This enum is a bandwidth-optimized counterpart to
/// [`proto::message::Client`], with nested enums and empty payloads
/// collapsed into distinct variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(crate = "proto::serde")]
#[repr(u32)]
pub(crate) enum ClientMessageFlat {
    Discovery = u32::from_le_bytes(*b"cdsc"),
    ConnectionSuccess = u32::from_le_bytes(*b"ccok"),
    ConnectionFailed = u32::from_le_bytes(*b"ccer"),
    ConnectionRefused = u32::from_le_bytes(*b"ccrf"),
    StartIO = u32::from_le_bytes(*b"csta"),
    StopIO = u32::from_le_bytes(*b"csto"),
    AudioHeader {
        // None of that confusing varint business
        #[serde(with = "postcard::fixint::le")]
        stream_idx: u32,
        #[serde(with = "postcard::fixint::le")]
        byte_idx: u64,
        #[serde(with = "postcard::fixint::le")]
        n_bytes: u32,
    } = u32::from_le_bytes(*b"caud"),
    Disconnect = u32::from_le_bytes(*b"cded"),
    Heartbeat = u32::from_le_bytes(*b"cliv"),
}

impl From<ClientMessageFlat> for proto::message::Client {
    fn from(v: ClientMessageFlat) -> Self {
        match v {
            ClientMessageFlat::Discovery => Self::Discovery,
            ClientMessageFlat::StartIO => Self::START_IO,
            ClientMessageFlat::StopIO => Self::STOP_IO,
            ClientMessageFlat::AudioHeader {
                stream_idx,
                byte_idx,
                n_bytes,
            } => Self::audio(proto::AudioMessageHeader {
                stream_idx,
                stream_msg: proto::AudioStreamMessageHeader { byte_idx, n_bytes },
            }),
            ClientMessageFlat::ConnectionSuccess => Self::CONN_SUCCESS,
            ClientMessageFlat::ConnectionFailed => Self::CONN_FAILED,
            ClientMessageFlat::ConnectionRefused => Self::CONN_REFUSED,
            ClientMessageFlat::Disconnect => Self::Disconnect,
            ClientMessageFlat::Heartbeat => Self::HEARTBEAT,
        }
    }
}

impl From<proto::message::Client> for ClientMessageFlat {
    #[inline(always)]
    fn from(v: proto::message::Client) -> Self {
        use proto::message::*;

        match v {
            Client::Discovery => Self::Discovery,
            Client::Connected(c) => match c {
                client::Connected::Control(ctrl) => match ctrl {
                    client::Control::RequestIOStateChange(s) => match s {
                        IOState::Start(()) => Self::StartIO,
                        IOState::Stop(()) => Self::StopIO,
                    },
                    client::Control::Heartbeat => Self::Heartbeat,
                },
                client::Connected::Audio(proto::AudioMessageHeader {
                    stream_idx,
                    stream_msg: proto::AudioStreamMessageHeader { byte_idx, n_bytes },
                }) => Self::AudioHeader {
                    stream_idx,
                    byte_idx,
                    n_bytes,
                },
            },
            Client::ConnectionResult(r) => match r {
                Ok(()) => Self::ConnectionSuccess,
                Err(e) => match e {
                    Error::Failure(()) => Self::ConnectionFailed,
                    Error::Refusal(()) => Self::ConnectionRefused,
                },
            },
            Client::Disconnect => Self::Disconnect,
        }
    }
}

/// Flattened wire representation of server-to-client messages.
///
/// This enum is a bandwidth-optimized counterpart to
/// [`proto::message::Server`], with nested enums collapsed into distinct variants.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(crate = "proto::serde")]
#[repr(u32)]
pub(crate) enum ServerMessageFlat {
    Connect(proto::format::StreamFormats) = u32::from_le_bytes(*b"scon"),
    StartIOFailed = u32::from_le_bytes(*b"saif"),
    StartIORefused = u32::from_le_bytes(*b"sair"),
    StartIOSuccess = u32::from_le_bytes(*b"sais"),
    StopIOFailed = u32::from_le_bytes(*b"soif"),
    StopIORefused = u32::from_le_bytes(*b"soir"),
    StopIOSuccess = u32::from_le_bytes(*b"sois"),
    AudioHeader {
        // None of that confusing varint business
        #[serde(with = "postcard::fixint::le")]
        stream_idx: u32,
        #[serde(with = "postcard::fixint::le")]
        byte_idx: u64,
        #[serde(with = "postcard::fixint::le")]
        n_bytes: u32,
    } = u32::from_le_bytes(*b"saud"),
    Disconnect = u32::from_le_bytes(*b"sded"),
    Heartbeat = u32::from_le_bytes(*b"sliv"),
}

impl From<ServerMessageFlat> for proto::message::Server {
    fn from(v: ServerMessageFlat) -> Self {

        match v {
            ServerMessageFlat::Connect(f) => Self::Connect(f),
            ServerMessageFlat::StartIOFailed => Self::START_IO_FAILED,
            ServerMessageFlat::StartIORefused => Self::START_IO_REFUSED,
            ServerMessageFlat::StartIOSuccess => Self::START_IO_OK,
            ServerMessageFlat::StopIOFailed => Self::STOP_IO_FAILED,
            ServerMessageFlat::StopIORefused => Self::STOP_IO_REFUSED,
            ServerMessageFlat::StopIOSuccess => Self::STOP_IO_OK,
            ServerMessageFlat::AudioHeader {
                stream_idx,
                byte_idx,
                n_bytes,
            } => Self::audio(proto::AudioMessageHeader {
                stream_idx,
                stream_msg: proto::AudioStreamMessageHeader { byte_idx, n_bytes },
            }),
            ServerMessageFlat::Disconnect => Self::Disconnect,
            ServerMessageFlat::Heartbeat => Self::HEARTBEAT,
        }
    }
}

impl From<proto::message::Server> for ServerMessageFlat {
    fn from(v: proto::message::Server) -> Self {
        use proto::message::*;

        match v {
            Server::Connect(f) => Self::Connect(f),
            Server::Connected(c) => match c {
                server::Connected::Control(ctrl) => match ctrl {
                    server::Control::IOStateChangeResult(s) => match s {
                        IOState::Start(r) => match r {
                            Ok(()) => Self::StartIOSuccess,
                            Err(e) => match e {
                                Error::Failure(_) => Self::StartIOFailed,
                                Error::Refusal(_) => Self::StartIORefused,
                            },
                        },
                        IOState::Stop(r) => match r {
                            Ok(()) => Self::StopIOSuccess,
                            Err(e) => match e {
                                Error::Failure(_) => Self::StopIOFailed,
                                Error::Refusal(_) => Self::StopIORefused,
                            },
                        },
                    },
                    server::Control::Heartbeat => Self::Heartbeat,
                },
                server::Connected::Audio(proto::AudioMessageHeader {
                    stream_idx,
                    stream_msg: proto::AudioStreamMessageHeader { byte_idx, n_bytes },
                }) => Self::AudioHeader {
                    stream_idx,
                    byte_idx,
                    n_bytes,
                },
            },
            Server::Disconnect => Self::Disconnect,
        }
    }
}

/// Encodes a client message into a [`std::io::Write`]
pub fn client_message_encode<W: std::io::Write>(
    m: proto::message::Client,
    w: W,
) -> postcard::Result<W> {
    postcard::to_io(&ClientMessageFlat::from(m), w)
}

/// Decodes a client message from a slice
pub fn client_message_decode(slice: &[u8]) -> postcard::Result<(proto::message::Client, &[u8])> {
    let mut d = postcard::Deserializer::from_bytes(slice);
    crate::ClientMessageFlat::deserialize(&mut d).map(|m| (m.into(), d.finalize().unwrap()))
}

/// Encodes a server message into a [`std::io::Read`]
pub fn server_message_encode<W: std::io::Write>(
    m: proto::message::Server,
    w: W,
) -> Result<W, postcard::Error> {
    postcard::to_io(&ServerMessageFlat::from(m), w)
}

/// Decodes a server message from a slice
pub fn server_message_decode(slice: &[u8]) -> postcard::Result<(proto::message::Server, &[u8])> {
    let mut d = postcard::Deserializer::from_bytes(slice);
    crate::ServerMessageFlat::deserialize(&mut d).map(|m| (m.into(), d.finalize().unwrap()))
}

/// Utility for converting a `postcard` error into a [`std::io::Error`].
///
/// This is primarily used at the UDP receive boundary, where deserialization
/// failures must be reported using IO–oriented error types.
#[inline(always)]
pub(crate) fn postcard_to_io_err(e: postcard::Error) -> std::io::Error {
    match e {
        postcard::Error::DeserializeUnexpectedEnd => std::io::ErrorKind::UnexpectedEof.into(),
        _ => std::io::ErrorKind::Other.into(),
    }
}

/// Returns `true` if the given IO error kind represents a timeout condition.
///
/// This treats both `WouldBlock` and `TimedOut` as timeout-equivalent.
#[inline(always)]
pub(crate) fn io_err_is_timeout(e: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind::*;
    [WouldBlock, TimedOut].contains(&e)
}

pub const AUDIO_STREAM_MESSAGE_HEADER_SIZE: usize = size_of::<u64>() + size_of::<u32>();
pub const AUDIO_MESSAGE_HEADER_SIZE: usize = AUDIO_STREAM_MESSAGE_HEADER_SIZE + size_of::<u32>();

/// Trait encapsulating the behavior of a synchronous (i.e. blocking) UDP socket.
///
/// We do this to allow easy integration of other, socket implementations, more flexible and
/// performant than those of the standard library, notably `socket2`
pub trait SyncUdpSock {
    fn send(&self, bytes: &[u8], dest_addr: core::net::SocketAddr) -> std::io::Result<()>;

    fn recv(
        &self,
        bytes: &mut [u8],
    ) -> std::io::Result<(usize, core::net::SocketAddr, std::time::Instant)>;

    fn set_recv_timeout(&self, timeout: Option<core::time::Duration>) -> std::io::Result<()>;
}

impl SyncUdpSock for std::net::UdpSocket {
    fn send(&self, bytes: &[u8], dest_addr: core::net::SocketAddr) -> std::io::Result<()> {
        self.send_to(bytes, dest_addr).and_then(|n| {
            (n == bytes.len())
                .then_some(())
                .ok_or(std::io::ErrorKind::FileTooLarge.into())
        })
    }

    fn recv(
        &self,
        bytes: &mut [u8],
    ) -> std::io::Result<(usize, core::net::SocketAddr, std::time::Instant)> {
        let (bytes_read, peer_addr) = self.recv_from(bytes)?;

        Ok((bytes_read, peer_addr, std::time::Instant::now()))
    }

    fn set_recv_timeout(&self, timeout: Option<core::time::Duration>) -> std::io::Result<()> {
        self.set_read_timeout(timeout)
    }
}
