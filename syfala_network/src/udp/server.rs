//! Server-side UDP network implementation.
//!
//! This module provides a thin UDP transport layer for servers implementing
//! the `syfala_proto` message model. It handles serialization, deserialization, and
//! basic receive loops, while delegating all protocol logic and state management to
//! user-provided callbacks.

use core::{convert::Infallible, net::SocketAddr};

/// A UDP server socket
///
/// This type encapsulates a UDP socket, used to communicate with one or more clients,
/// and the stream formats advertised by the server. It provides facilities for sending
/// messages to clients, but deliberately does **not** expose a public receive API.
///
/// Message reception is driven through the [`ServerState`] trait, which defines
/// the server’s receive loop and callback behavior.
///
/// The server itself is agnostic to whether messages are sent via unicast,
/// multicast, or broadcast addresses.
#[derive(Debug)]
pub struct ServerSocket {
    sock: std::net::UdpSocket,
}

impl ServerSocket {
    /// Creates a new server backed by the given UDP socket and stream formats.
    ///
    /// # Note
    ///
    /// The provided `formats` are advertised to clients during connection
    /// establishment and remain fixed for the lifetime of the server.
    #[inline(always)]
    pub fn new(sock: std::net::UdpSocket) -> Self {
        Self { sock }
    }

    #[inline]
    pub fn send_packet(&self, bytes: &[u8], dest_addr: SocketAddr) -> std::io::Result<()> {
        let res = self.sock.send_to(bytes, dest_addr);

        res.and_then(|n| {
            (n == bytes.len())
                .then_some(())
                .ok_or(std::io::ErrorKind::FileTooLarge.into())
        })
    }

    /// Serializes and sends a server message to the specified destination address.
    ///
    /// The message is encoded using [`postcard`] into the provided buffer and then
    /// sent as a single UDP datagram.
    ///
    /// The destination address may be unicast, multicast, or broadcast.
    #[inline]
    pub fn send_msg(
        &self,
        message: syfala_proto::message::Server,
        client_addr: SocketAddr,
        buf: &mut [u8],
    ) -> std::io::Result<()> {
        crate::server_message_encode(message, buf)
            .map_err(crate::postcard_to_io_err)
            .and_then(|s| self.send_packet(s, client_addr))
    }

    /// Receives and deserializes a client message from the underlying socket.
    ///
    /// On success, returns the sender’s socket address and an optional decoded
    /// protocol message.
    ///
    /// If a datagram is received but cannot be parsed as a valid protocol message,
    /// the returned `Option` will be `None`.
    #[inline]
    fn recv<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> std::io::Result<(
        SocketAddr,
        Option<(syfala_proto::message::Client, &'a [u8])>,
    )> {
        self.sock.recv_from(buf).map(|(n, client_addr)| {
            let buf = &buf[..n];

            (client_addr, crate::client_message_decode(buf).ok())
        })
    }
}

/// Encapsulates server-side protocol state and message handling.
/// 
/// Implementors of this trait define how the server reacts to incoming client
/// messages. The provided [`start`](ServerState::start) method runs a blocking receive
/// loop and dispatches messages to [`on_message`](ServerState::on_message).
/// 
/// This design allows applications to cleanly separate networking concerns from
/// higher-level protocol logic.
pub trait ServerState {
    /// Called on every received datagram.
    ///
    /// The `message` parameter is `None` if the datagram could not be decoded as a
    /// valid protocol message.
    fn on_message(
        &mut self,
        server: &ServerSocket,
        client_addr: core::net::SocketAddr,
        message: Option<(syfala_proto::message::Client, &[u8])>,
    ) -> std::io::Result<()>;

    /// Starts the server receive loop.
    /// 
    /// This function blocks indefinitely, receiving datagrams and invoking
    /// [`on_message`](ServerState::on_message) for each one.
    /// 
    /// The function only returns if a non-recoverable I/O error occurs.
    fn start(&mut self, server: &ServerSocket) -> std::io::Result<Infallible> {
        let mut buf = [0; 5000];

        loop {
            let res = server.recv(&mut buf);

            let (peer_addr, maybe_msg) = match res {
                Ok(r) => r,
                Err(e) if crate::io_err_is_timeout(e.kind()) => continue,
                Err(e) => return Err(e),
            };

            self.on_message(server, peer_addr, maybe_msg)?;
        }
    }
}
