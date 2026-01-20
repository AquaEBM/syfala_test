use core::{convert::Infallible, net::SocketAddr};

/// A client.
/// 
/// Encapsulates sending and receiving messages, as a client, over a UDP socket.
#[derive(Debug)]
pub struct Client {
    sock: std::net::UdpSocket,
}

impl Client {
    #[inline(always)]
    pub fn new(sock: std::net::UdpSocket) -> Self {
        Self { sock }
    }

    /// Serializes and sends the given `message` to the address provided by `dest_addr`
    /// 
    /// Note that `dest_addr` doesn't care if it's a multi, broad, or uni-cast address.
    #[inline(always)]
    pub fn send(
        &self,
        message: syfala_proto::message::Client<'_>,
        dest_addr: SocketAddr,
        buf: &mut [u8],
    ) -> std::io::Result<()> {
        let left = syfala_proto::postcard::to_slice(&crate::ClientMessageFlat::from(message), buf)
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

    /// Deserializes and receives a server message from the internal socket.
    /// 
    /// Note that if a datagram was found, but couldn't be parsed as one of our messages,
    /// then the `Option` is `None`.
    #[inline(always)]
    fn recv<'a>(
        &self,
        buf: &'a mut [u8],
    ) -> std::io::Result<(SocketAddr, Option<syfala_proto::message::Server<'a>>)> {
        self.sock.recv_from(buf).map(|(n, server)| {
            let buf = &buf[..n];
            (
                server,
                syfala_proto::postcard::from_bytes::<'a, crate::ServerMessageFlat>(buf)
                    .ok()
                    .map(Into::into),
            )
        })
    }
}

/// Encapsulates the state and callbacks a UDP client calls on reception of messages from servers
pub trait ClientState {
    /// The callback to be called on each message
    fn on_message(
        &mut self,
        client: &Client,
        addr: core::net::SocketAddr,
        message: Option<syfala_proto::message::Server<'_>>,
    ) -> std::io::Result<()>;

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
