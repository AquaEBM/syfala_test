//! Client-side UDP network implementation.
//!
//! This module provides a thin UDP transport layer for clients implementing
//! the `syfala_proto` message model. It handles serialization, deserialization, and
//! basic receive loops, while delegating all protocol logic and state management to
//! user-provided callbacks.

use core::{convert::Infallible, net::SocketAddr};

#[cfg(feature = "generic")]
pub mod generic;

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
pub struct ClientSocket<T> {
    sock: T,
}

impl<T> ClientSocket<T> {
    /// Creates a new server backed by the given UDP socket.
    #[inline(always)]
    pub fn new(sock: T) -> Self {
        Self { sock }
    }
}

impl<T: crate::SyncUdpSock> ClientSocket<T> {
    #[inline]
    pub fn send_raw_packet(&self, bytes: &[u8], dest_addr: SocketAddr) -> std::io::Result<()> {
        self.sock.send(bytes, dest_addr)
    }

    /// Serializes and sends a client message to the specified destination address.
    ///
    /// The message is encoded using [`postcard`] into the provided buffer and then
    /// sent as a single UDP datagram.
    ///
    /// The destination address may be unicast, multicast, or broadcast.
    #[inline(always)]
    pub fn send_msg(
        &self,
        message: syfala_proto::message::Client,
        server_addr: SocketAddr,
        buf: &mut [u8],
    ) -> std::io::Result<()> {
        crate::client_message_encode(message, buf)
            .map_err(crate::postcard_to_io_err)
            .and_then(|s| self.send_raw_packet(s, server_addr))
    }

    pub fn set_recv_timeout(&self, timeout: Option<core::time::Duration>) -> std::io::Result<()> {
        self.sock.set_recv_timeout(timeout)
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
    ) -> std::io::Result<(
        SocketAddr,
        std::time::Instant,
        Option<(syfala_proto::message::Server, &'a [u8])>,
    )> {
        self.sock.recv(buf).map(|(n, server, timestamp)| {
            let buf = &buf[..n];

            (server, timestamp, crate::server_message_decode(buf).ok())
        })
    }

    #[inline]
    pub fn start_discovery_beacon(
        &self,
        period: core::time::Duration,
        dest_addr: SocketAddr,
    ) -> std::io::Result<core::convert::Infallible> {
        // TODO: calculate the payload size and allocate exactly that
        // This is currently an experimentat feature of postcard
        let mut disc_packet_buf = std::io::Cursor::new([0; 2000]);

        // we encode our discovery message only once
        crate::client_message_encode(
            crate::proto::message::Client::Discovery,
            &mut disc_packet_buf,
        )
        .map_err(crate::postcard_to_io_err)?;

        let payload_size = usize::try_from(disc_packet_buf.position()).unwrap();

        let buf = &disc_packet_buf.get_ref()[..payload_size];

        loop {
            let res = self.send_raw_packet(buf, dest_addr);

            match res {
                Err(e) if crate::io_err_is_timeout(e.kind()) => continue,
                Err(e) => return Err(e),
                _ => (),
            };

            std::thread::sleep(period);
        }
    }
}

/// Encapsulates client-side state and message handling.
///
/// Implementors of this trait define how the client reacts to incoming server
/// messages. The provided [`start`](ClientState::start) method runs a blocking receive
/// loop and dispatches messages to [`on_message`](ClientState::on_message).
///
/// This design allows applications to cleanly separate networking concerns from
/// higher-level protocol logic.
pub trait Client {
    /// Called on every received datagram.
    ///
    /// The `message` parameter is `None` if the datagram could not be decoded as a
    /// valid protocol message.
    fn on_message(
        &mut self,
        client: &ClientSocket<impl crate::SyncUdpSock>,
        server_addr: core::net::SocketAddr,
        timestamp: std::time::Instant,
        message: Option<(syfala_proto::message::Server, &[u8])>,
    ) -> std::io::Result<()>;

    fn on_timeout(&mut self, client: &ClientSocket<impl crate::SyncUdpSock>) -> std::io::Result<()>;

    /// Starts the client receive loop
    ///
    /// This function blocks indefinitely, receiving datagrams and invoking
    /// [`on_message`](ClientState::on_message) for each one.
    ///
    /// The function only returns if a non-recoverable I/O error occurs.
    fn start(&mut self, client: &ClientSocket<impl crate::SyncUdpSock>) -> std::io::Result<Infallible> {
        let mut buf = [0; 5000];

        loop {
            let res = client.recv(&mut buf);

            // don't return on timeout errors...
            match res {
                Ok((addr, timestamp, maybe_msg)) => {
                    self.on_message(client, addr, timestamp, maybe_msg)?
                }
                Err(e) if crate::io_err_is_timeout(e.kind()) => self.on_timeout(client)?,
                Err(e) => return Err(e),
            };
        }
    }
}