//! Implementation of the message model defined in the `syfala_proto` crate.
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
//! communication layer for the message model described in `syfala_proto`.

pub mod server;
pub mod client;
pub use syfala_proto;
pub use postcard;
pub use serde;

use serde::{Deserialize, Serialize};

// Internal types with flat enum representations so serde and postcard don't waste bandwidth
// serializing/deserializing nested enums.
// 
// These enums remove structural nesting present in `syfala_proto::message` and instead
// encode each meaningful message variant directly as a single discriminant. This mirrors
// the kind of layout flattening performed by the Rust compiler for in-memory enums, but
// applied explicitly at the wire level.
// 
// Callers of this library never see these types. They are used exclusively at the
// serialization boundary in `{Client,Server}::{send,recv}`, and are converted to and from
// the public protocol message types automatically.
// 
// See the relevant comment in `syfala_proto::message` for more.

// ------

/// Flattened wire representation of client-to-server messages.
///
/// This enum is a bandwidth-optimized counterpart to
/// [`syfala_proto::message::Client`], with nested enums and empty payloads
/// collapsed into distinct variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum ClientMessageFlat<'a> {
    Connect,
    ConnectionFailed,
    ConnectionRefused,
    StartIO,
    StopIO,
    Audio(#[serde(borrow)] syfala_proto::AudioData<'a>),
}

impl<'a> From<syfala_proto::message::Client<'a>> for ClientMessageFlat<'a> {
    #[inline(always)]
    fn from(v: syfala_proto::message::Client<'a>) -> Self {
        use syfala_proto::*;

        match v {
            message::Client::Connect => Self::Connect,
            message::Client::Connected(c) => match c {
                message::client::Connected::Control(ctrl) => match ctrl {
                    message::client::Control::RequestIOStateChange(s) => match s {
                        message::IOState::Start(()) => Self::StartIO,
                        message::IOState::Stop(()) => Self::StopIO,
                    },
                },
                message::client::Connected::Audio(a) => Self::Audio(a),
            },
            message::Client::ConnectionError(e) => match e {
                message::Error::Failure => Self::ConnectionFailed,
                message::Error::Refusal => Self::ConnectionRefused,
            },
        }
    }
}

impl<'a> From<ClientMessageFlat<'a>> for syfala_proto::message::Client<'a> {
    fn from(v: ClientMessageFlat<'a>) -> Self {
        use syfala_proto::*;
        match v {
            ClientMessageFlat::Connect => Self::Connect,
            ClientMessageFlat::ConnectionFailed => Self::ConnectionError(message::Error::Failure),
            ClientMessageFlat::ConnectionRefused => Self::ConnectionError(message::Error::Refusal),
            ClientMessageFlat::StartIO => Self::Connected(message::client::Connected::Control(
                message::client::Control::RequestIOStateChange(message::IOState::Start(())),
            )),
            ClientMessageFlat::StopIO => Self::Connected(message::client::Connected::Control(
                message::client::Control::RequestIOStateChange(message::IOState::Stop(())),
            )),
            ClientMessageFlat::Audio(a) => Self::Connected(message::client::Connected::Audio(a)),
        }
    }
}

/// Flattened wire representation of server-to-client messages.
///
/// This enum is a bandwidth-optimized counterpart to
/// [`syfala_proto::message::Server`], with nested enums collapsed into distinct variants.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub(crate) enum ServerMessageFlat<'a> {
    Connect(syfala_proto::format::StreamFormats),
    ConnectionFailed,
    ConnectionRefused,
    StartIOFailed,
    StartIORefused,
    StartIOSuccess,
    StopIOFailed,
    StopIORefused,
    StopIOSuccess,
    Audio(#[serde(borrow)] syfala_proto::AudioData<'a>),
}

impl<'a> From<syfala_proto::message::Server<'a>> for ServerMessageFlat<'a> {
    fn from(v: syfala_proto::message::Server<'a>) -> Self {
        use syfala_proto::*;

        match v {
            message::Server::Connect(f) => Self::Connect(f),
            message::Server::ConnectionError(e) => match e {
                message::Error::Failure => Self::ConnectionFailed,
                message::Error::Refusal => Self::ConnectionRefused,
            },
            message::Server::Connected(c) => match c {
                message::server::Connected::Control(ctrl) => match ctrl {
                    message::server::Control::IOStateChangeResult(s) => match s {
                        message::IOState::Start(r) => match r {
                            Ok(()) => Self::StartIOSuccess,
                            Err(e) => match e {
                                message::Error::Failure => Self::StartIOFailed,
                                message::Error::Refusal => Self::StartIORefused,
                            },
                        },
                        message::IOState::Stop(r) => match r {
                            Ok(()) => Self::StopIOSuccess,
                            Err(e) => match e {
                                message::Error::Failure => Self::StopIOFailed,
                                message::Error::Refusal => Self::StopIORefused,
                            },
                        },
                    },
                },
                message::server::Connected::Audio(a) => Self::Audio(a),
            },
        }
    }
}

impl<'a> From<ServerMessageFlat<'a>> for syfala_proto::message::Server<'a> {
    fn from(v: ServerMessageFlat<'a>) -> Self {
        use syfala_proto::*;

        match v {
            ServerMessageFlat::Connect(f) => Self::Connect(f),
            ServerMessageFlat::ConnectionFailed => Self::ConnectionError(message::Error::Failure),
            ServerMessageFlat::ConnectionRefused => Self::ConnectionError(message::Error::Refusal),
            ServerMessageFlat::StartIOFailed => Self::Connected(
                message::server::Connected::Control(message::server::Control::IOStateChangeResult(
                    message::IOState::Start(Err(message::Error::Failure)),
                )),
            ),
            ServerMessageFlat::StartIORefused => Self::Connected(
                message::server::Connected::Control(message::server::Control::IOStateChangeResult(
                    message::IOState::Start(Err(message::Error::Refusal)),
                )),
            ),
            ServerMessageFlat::StartIOSuccess => {
                Self::Connected(message::server::Connected::Control(
                    message::server::Control::IOStateChangeResult(message::IOState::Start(Ok(()))),
                ))
            }
            ServerMessageFlat::StopIOFailed => Self::Connected(
                message::server::Connected::Control(message::server::Control::IOStateChangeResult(
                    message::IOState::Stop(Err(message::Error::Failure)),
                )),
            ),
            ServerMessageFlat::StopIORefused => Self::Connected(
                message::server::Connected::Control(message::server::Control::IOStateChangeResult(
                    message::IOState::Stop(Err(message::Error::Refusal)),
                )),
            ),
            ServerMessageFlat::StopIOSuccess => {
                Self::Connected(message::server::Connected::Control(
                    message::server::Control::IOStateChangeResult(message::IOState::Stop(Ok(()))),
                ))
            }
            ServerMessageFlat::Audio(a) => Self::Connected(message::server::Connected::Audio(a)),
        }
    }
}

// ----

/// Utility for converting a `postcard` error into a [`std::io::Error`].
///
/// This is primarily used at the UDP receive boundary, where deserialization
/// failures must be reported using I/O–oriented error types.
#[inline(always)]
pub(crate) fn postcard_to_io_err(e: postcard::Error) -> std::io::Error {
    match e {
        postcard::Error::DeserializeUnexpectedEnd => {
            std::io::ErrorKind::UnexpectedEof.into()
        }
        _ => std::io::ErrorKind::Other.into(),
    }
}

/// Returns `true` if the given I/O error kind represents a timeout condition.
///
/// This treats both `WouldBlock` and `TimedOut` as timeout-equivalent, which
/// is useful when working with non-blocking or socket-based transports.
#[inline(always)]
pub(crate) fn io_err_is_timeout(e: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind::*;
    [WouldBlock, TimedOut].contains(&e)
}
