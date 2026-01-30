//! UDP client state machine for managing multiple servers.
//!
//! This module provides a synchronous (blocking), client that tracks server connections,
//! handles IO state (start/stop requests active audio), and manages per-server deadlines
//! for timeout detection
//!
//! The implementation relies on typestate-based transitions and explicit
//! state machines to model the server IO lifecycle.
//!
//! Socket timeouts are used to periodically poll server deadlines and
//! disconnect inactive servers.

mod state;
use core::cmp;
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use rustc_hash::FxBuildHasher;
use state::{
    ClientContext, IOActiveContext, IOInactiveContext, IOStartPendingContext, IOStopPendingConxtext,
};
use syfala_proto::message::{Client, Error, IOState, Server, client, server};

/// Hash map storing per-server state, keyed by socket address.
type ServerMap<V> = rustc_hash::FxHashMap<core::net::SocketAddr, V>;

/// Priority queue keyed by server address, with associated priority value.
type ServerPQ<V> = priority_queue::PriorityQueue<core::net::SocketAddr, V, FxBuildHasher>;

/// Duration after which a server is considered disconnected if no valid
/// message is received. Each successfully handled message refreshes the deadline.
const CONN_TIMEOUT: core::time::Duration = core::time::Duration::from_millis(600);
/// the delay between subsequent retries of client request polls
const REQUEST_POLL_PERIOD: core::time::Duration = core::time::Duration::from_millis(10);

/// Temporary stack buffer size used to encode outgoing protocol messages.
const ENCODE_BUF_LEN: usize = 2000;

// Note: Comments that should be logs are marked with (*)

/// Represents the IO state machine for a connected server.
///
/// This enum wraps the different typestate objects representing:
/// - Inactive IO
/// - Pending start request
/// - Active IO
/// - Pending stop request
///
/// All state transitions are driven by incoming messages or application requests.
enum ServerIOState<Cx: ClientContext + ?Sized> {
    Inactive(state::Inactive<Cx>),
    PendingStart(state::StartPending<Cx>),
    Active(state::Active<Cx>),
    PendingStop(state::StopPending<Cx>),
}

// TODO: replace all comments marked with (*) with logs

impl<Cx: ClientContext + ?Sized> ServerIOState<Cx> {
    /// Handles an incoming `Server::Connected` message.
    ///
    /// Dispatches control and audio messages to the current state object,
    /// adn performs state transitions and callbacks where needed.
    ///
    /// Invalid messages for the current state are ignored, but may be logged.
    fn on_msg(
        &mut self,
        addr: core::net::SocketAddr,
        cx: &mut Cx,
        sock: &super::ClientSocket<impl crate::SyncUdpSock>,
        msg: (server::Connected, &[u8]),
        timestamp: std::time::Instant,
    ) -> std::io::Result<()> {
        let (msg, rem_buf) = msg;

        let mut encode_buf = [0; 2000];

        use server::Connected;

        match msg {
            // Control-plane messages related to IO state transitions.
            Connected::Control(server::Control::IOStateChangeResult(r)) => match r {
                // Server acknowledged an IO start request.
                IOState::Start(r) => match r {
                    Ok(()) => replace_with_or_abort(self, |s| match s {
                        Self::PendingStart(s) => Self::Active(s.start_io(cx)),
                        a => {
                            // (*) not waiting for IO start
                            a
                        }
                    }),
                    Err(e) => match e {
                        // Temporary failure: retry start request.
                        Error::Failure(()) => match self {
                            Self::PendingStart(s) => {
                                s.start_io_failed(cx);
                                sock.send_msg(
                                    Client::Connected(client::Connected::Control(
                                        client::Control::RequestIOStateChange(IOState::Start(())),
                                    )),
                                    addr,
                                    &mut encode_buf,
                                )?;
                                // (*) io start failed, retrying...
                            }
                            _ => {
                                // (*) not waiting for IO to start
                            }
                        },
                        // Permanent refusal: notify callbacks and do not retry.
                        Error::Refusal(()) => replace_with_or_abort(self, |s| match s {
                            Self::PendingStart(s) => Self::Inactive(s.start_io_refused(cx)),
                            a => {
                                // (*) not waiting for IO start
                                a
                            }
                        }),
                    },
                },

                // Server acknowledged an IO stop request.
                IOState::Stop(r) => match r {
                    Ok(()) => replace_with_or_abort(self, |s| match s {
                        Self::PendingStop(s) => Self::Inactive(s.stop_io(cx)),
                        a => {
                            // (*) not waiting for io stop
                            a
                        }
                    }),
                    Err(e) => match e {
                        // Temporary failure: retry stop request.
                        Error::Failure(()) => match self {
                            Self::PendingStop(s) => {
                                s.stop_io_failed(cx);
                                sock.send_msg(
                                    Client::Connected(client::Connected::Control(
                                        client::Control::RequestIOStateChange(IOState::Stop(())),
                                    )),
                                    addr,
                                    &mut encode_buf,
                                )?
                            }
                            _ => {
                                // (*) not waiting for IO stop
                            }
                        },
                        // Permanent refusal: notify callbacks.
                        Error::Refusal(()) => replace_with_or_abort(self, |s| match s {
                            Self::PendingStop(s) => Self::Active(s.stop_io_refused(cx)),
                            a => {
                                // (*) not waiting for IO stop
                                a
                            }
                        }),
                    },
                },
            },

            // timeout updating is done outside of this function
            Connected::Control(server::Control::Heartbeat) => (),

            Connected::Audio(header) => match self {
                ServerIOState::Active(s) => s.on_audio(cx, timestamp, header, rem_buf),
                _ => {
                    // (*) audio IO inactive
                }
            },
        }

        Ok(())
    }
}

/// Generic client managing connections to multiple servers.
///
/// This type owns the client context implementing connection and IO callbacks
///
/// This also maintains a priority queue of per-server connection timeout deadlines.
///
/// It implements the [`Client`] so that it can be driven by a blocking UDP receive loop.
pub struct GenericClient<C: ClientContext> {
    /// Priority queue tracking next timeout per server.
    ///
    /// We use [`core::cmp::Reverse`] here to ensure the _earliest_ instant
    /// has the _highest_ priority
    deadlines: ServerPQ<cmp::Reverse<std::time::Instant>>,
    /// Per-server state machine storage.
    servers: ServerMap<ServerIOState<C>>,
    retry_deadline: Option<std::time::Instant>,
    /// User-provided callbacks defining connection, IO, and audio behavior.
    callbacks: C,
}

impl<C: ClientContext> GenericClient<C> {
    /// Creates a new client instance with the given context.
    ///
    /// Initially, no servers are connected, and the deadline queue is empty.
    #[inline(always)]
    pub const fn new(callbacks: C) -> Self {
        Self {
            callbacks,
            deadlines: ServerPQ::with_hasher(FxBuildHasher),
            servers: ServerMap::with_hasher(FxBuildHasher),
            retry_deadline: None,
        }
    }

    /// Handles an incoming server connection request.
    ///
    /// If the server is not already connected, invokes `connect` on the
    /// client context to determine whether the connection is accepted.
    /// Sends a `Client::ConnectionResult` back to the server accordingly.
    fn on_server_connect_request(
        &mut self,
        sock: &super::ClientSocket<impl crate::SyncUdpSock>,
        addr: core::net::SocketAddr,
        formats: syfala_proto::format::StreamFormats,
        encode_buf: &mut [u8],
    ) -> std::io::Result<()> {
        if !self.servers.contains_key(&addr) {
            match self.callbacks.connect(addr, formats) {
                Ok(state) => {
                    self.servers.insert(addr, ServerIOState::Inactive(state));
                    sock.send_msg(Client::ConnectionResult(Ok(())), addr, encode_buf)?;
                    // (*) connection success
                }
                Err(e) => {
                    sock.send_msg(Client::ConnectionResult(Err(e)), addr, encode_buf)?;
                    // (*) connection failed/rejected
                }
            }
        } else {
            // (*) server already connected
        }

        Ok(())
    }

    /// Dispatches a decoded server message and, maybe, updates the corresponding state machine.
    ///
    /// Also refreshes the server's deadline if it is still connected.
    fn on_decoded_message(
        &mut self,
        sock: &super::ClientSocket<impl crate::SyncUdpSock>,
        addr: core::net::SocketAddr,
        timestamp: std::time::Instant,
        msg: (syfala_proto::message::Server, &[u8]),
    ) -> std::io::Result<()> {
        let mut buf = [0; ENCODE_BUF_LEN];

        let (msg, rem_buf) = msg;

        match msg {
            Server::Connect(formats) => {
                self.on_server_connect_request(sock, addr, formats, &mut buf)?;
            }
            Server::Connected(msg) => {
                if let Some(state) = self.servers.get_mut(&addr) {
                    state.on_msg(addr, &mut self.callbacks, sock, (msg, rem_buf), timestamp)?;
                }
            }
            Server::Disconnect => match self.servers.remove(&addr) {
                Some(_s) => {
                    self.deadlines.remove(&addr).unwrap();
                    // (*) successfully disconnected from server
                }
                None => {
                    // (*) no connected server at that address
                }
            },
        }

        if self.servers.contains_key(&addr) {
            // not that push _replaces_ the corresponding entry if it already exists, so the
            // number of elements in deadlines is always exactly the number of connected servers
            self.deadlines.push(
                addr,
                cmp::Reverse(timestamp.checked_add(CONN_TIMEOUT).unwrap()),
            );
        }

        Ok(())
    }

    /// Handles an incoming UDP message (or lack thereof).
    fn on_message(
        &mut self,
        sock: &super::ClientSocket<impl crate::SyncUdpSock>,
        addr: core::net::SocketAddr,
        timestamp: std::time::Instant,
        maybe_msg: Option<(syfala_proto::message::Server, &[u8])>,
    ) -> std::io::Result<()> {
        match maybe_msg {
            Some(msg) => self.on_decoded_message(sock, addr, timestamp, msg)?,
            None => self.callbacks.unknown_message(addr),
        }

        Ok(())
    }

    /// Handles a socket receive timeout.
    ///
    /// Expires all servers whose deadlines have elapsed, removes them
    /// from the map, and resets the socket receive timeout to the next
    /// earliest deadline if any.
    fn on_timeout(
        &mut self,
        sock: &super::ClientSocket<impl crate::SyncUdpSock>,
    ) -> std::io::Result<()> {
        let now = std::time::Instant::now();

        // Expire all overdue servers
        while let Some((addr, _)) = self
            .deadlines
            .pop_if(|_, cmp::Reverse(deadline)| *deadline <= now)
        {
            self.servers.remove(&addr).unwrap();
        }

        // Manage incoming application requests, and retrying pending server requests
        let mut encode_buf = [0; 200];

        for (addr, state) in &mut self.servers {
            replace_with_or_abort_and_return(state, |s| match s {
                ServerIOState::Inactive(s) => match s.poll_start_io(&mut self.callbacks) {
                    Ok(s) => {
                        // (*) start IO requested by client for the server at addr
                        (
                            sock.send_msg(Client::START_IO, *addr, &mut encode_buf),
                            ServerIOState::PendingStart(s),
                        )
                    }
                    Err(s) => (Ok(()), ServerIOState::Inactive(s)),
                },
                ServerIOState::Active(s) => match s.poll_stop_io(&mut self.callbacks) {
                    Ok(s) => {
                        // (*) stop IO requested by client for the server at addr
                        (
                            sock.send_msg(Client::START_IO, *addr, &mut encode_buf),
                            ServerIOState::PendingStop(s),
                        )
                    }
                    Err(s) => (Ok(()), ServerIOState::Active(s)),
                },
                state => (Ok(()), state),
            })?;
        }

        let min_timeout = REQUEST_POLL_PERIOD.min(CONN_TIMEOUT);

        sock.set_recv_timeout(
            self.deadlines
                .peek()
                .map(|(_, cmp::Reverse(next))| next.saturating_duration_since(now))
                .map(|t| t.min(REQUEST_POLL_PERIOD).min(CONN_TIMEOUT)),
                
        )?;

        Ok(())
    }
}
