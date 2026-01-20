//! Client-side UDP network implementation.
//!
//! This module provides a thin UDP-based transport layer for clients implementing
//! the `syfala_proto` message model. It handles serialization, deserialization, and
//! basic receive loops, while delegating all protocol logic and state management to
//! user-provided callbacks.

use core::{convert::Infallible, net::SocketAddr};

/// A UDP server.
/// 
/// This type encapsulates a UDP socket, used to communicate with one or more servers.
///  It provides facilities for sending messages to servers, but deliberately
/// does **not** expose a public receive API.
/// 
/// Message reception is driven through the [`ClientState`] trait, which defines
/// the client's receive loop and callback behavior.
/// 
/// The client itself is agnostic to whether messages are sent via unicast,
/// multicast, or broadcast addresses.
#[derive(Debug)]
pub struct Client {
    sock: std::net::UdpSocket,
}

impl Client {
    /// Creates a new server backed by the given UDP socket.
    #[inline(always)]
    pub fn new(sock: std::net::UdpSocket) -> Self {
        Self { sock }
    }

    /// Serializes and sends a client message to the specified destination address.
    ///
    /// The message is encoded using [`postcard`] into the provided buffer and then
    /// sent as a single UDP datagram.
    ///
    /// The destination address may be unicast, multicast, or broadcast.
    #[inline(always)]
    pub fn send(
        &self,
        message: syfala_proto::message::Client<'_>,
        dest_addr: SocketAddr,
        buf: &mut [u8],
    ) -> std::io::Result<()> {
        let left = postcard::to_slice(&crate::ClientMessageFlat::from(message), buf)
            .map_err(crate::postcard_to_io_err)?
            .len();

        let ser_len = buf.len().strict_sub(left);

        let res = self.sock.send_to(&mut buf[..ser_len], dest_addr);

        res.and_then(|n| {
            (n == ser_len)
                .then_some(())
                .ok_or(std::io::ErrorKind::FileTooLarge.into())
        })
    }

    /// Receives and deserializes a server message from the underlying socket.
    ///
    /// On success, returns the sender’s socket address and an optional decoded
    /// protocol message.
    ///
    /// If a datagram is received but cannot be parsed as a valid protocol message,
    /// the `Option` will be `None`.
    #[inline(always)]
    fn recv<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> std::io::Result<(SocketAddr, Option<syfala_proto::message::Server<'a>>)> {
        self.sock.recv_from(buf).map(|(n, server)| {
            let buf = &buf[..n];
            (
                server,
                postcard::from_bytes::<'a, crate::ServerMessageFlat>(buf)
                    .ok()
                    .map(Into::into),
            )
        })
    }
}

/// Encapsulates client-side protocol state and message handling.
/// 
/// Implementors of this trait define how the client reacts to incoming server
/// messages. The provided [`start`](ClientState::start) method runs a blocking receive
/// loop and dispatches messages to [`on_message`](ClientState::on_message).
/// 
/// This design allows applications to cleanly separate networking concerns from
/// higher-level protocol logic.
pub trait ClientState {
    /// Called on every received datagram.
    ///
    /// The `message` parameter is `None` if the datagram could not be decoded as a
    /// valid protocol message.
    fn on_message(
        &mut self,
        client: &Client,
        addr: core::net::SocketAddr,
        message: Option<syfala_proto::message::Server<'_>>,
    ) -> std::io::Result<()>;

    /// Starts the client receive loop.
    ///
    /// This function blocks indefinitely, receiving datagrams and invoking
    /// [`on_message`](ClientState::on_message) for each one.
    ///
    /// The function only returns if a non-recoverable I/O error occurs.
    fn start(&mut self, client: &Client) -> std::io::Result<Infallible> {
        let mut buf = [0; 5000];

        loop {
            let res = client.recv(&mut buf);

            // don't return on timeout errors...
            let (peer_addr, maybe_msg) = match res {
                Ok(r) => r,
                Err(e) if crate::io_err_is_timeout(e.kind()) => continue,
                Err(e) => return Err(e),
            };

            self.on_message(client, peer_addr, maybe_msg)?;
        }
    }
}
