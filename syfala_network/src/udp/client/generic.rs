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

use core::cmp;

use rustc_hash::FxBuildHasher;

/// Hash map storing per-server state, keyed by socket address.
type ServerMap<V> = rustc_hash::FxHashMap<core::net::SocketAddr, V>;

/// Priority queue keyed by server address, with associated priority value.
/// In this client, the priority is a `Reverse<Instant>` representing the next
/// timeout for the server.
type ServerPQ<V> = priority_queue::PriorityQueue<core::net::SocketAddr, V, FxBuildHasher>;

use replace_with::replace_with_or_abort;

use syfala_proto::message::{Client, Error, IOState, Server, client, server};

/// Duration after which a server is considered disconnected if no valid
/// message is received. Each successfully handled message refreshes the deadline.
const CONN_TIMEOUT: core::time::Duration = core::time::Duration::from_millis(600);
const MAX_SOCKET_TIMEOUT: core::time::Duration = core::time::Duration::from_millis(10);

/// Temporary stack buffer size used to encode outgoing protocol messages.
const ENCODE_BUF_LEN: usize = 2000;

/// Type alias representing the `Inactive` IO state for a given client context.
///
/// Resolves to the associated `IOInactive` type of the `ClientContext`.
type Inactive<Cx> = <Cx as ClientContext>::IOInactive;

/// Type alias representing the `StartPending` IO state for a given client context.
///
/// Resolves to the associated `IOStartPending` type of the `Inactive` state.
type StartPending<Cx> = <Inactive<Cx> as IOInactiveContext>::IOStartPending;

/// Type alias representing the `Active` IO state for a given client context.
///
/// Resolves to the associated `IOActive` type of the `StartPending` state.
type Active<Cx> = <StartPending<Cx> as IOStartPendingContext>::IOActive;

/// Type alias representing the `StopPending` IO state for a given client context.
///
/// Resolves to the associated `IOStopPending` type of the `Active` state.
type StopPending<Cx> = <Active<Cx> as IOActiveContext>::IOStopPending;

/// Represents the global client context which contains callbacks called on various
/// client-size events
///
/// This trait is implemented by the "application layer" client object, and provides:
/// - The ability to handle new server connections, and return a nother callback manager
/// for said connection
pub trait ClientContext {
    /// A newly connected server with inactive IO.
    type IOInactive: IOInactiveContext<Context = Self>;

    /// Invoked when a new server connection is requested.
    ///
    /// Returns:
    /// - `Ok(IOInactive)` if the connection is accepted, allowing further IO state transitions
    /// - `Err(Error)` if the connection is rejected or fails
    fn connect(
        &mut self,
        addr: core::net::SocketAddr,
        stream_formats: syfala_proto::format::StreamFormats,
    ) -> Result<Self::IOInactive, syfala_proto::message::Error>;
}

/// Typestate representing a server with inactive IO.
///
/// Provides a method to poll whether the application wishes to start IO,
/// returning a `StartPending` state if so.
pub trait IOInactiveContext: Sized {
    /// The parent client context associated with this inactive state.
    type Context;

    /// The typestate representing a pending IO start request.
    type IOStartPending: IOStartPendingContext<Context = Self::Context>;

    /// Polls whether the application requests starting IO.
    ///
    /// - Returns `Ok(IOStartPending)` if a start request was made
    /// - Returns `Err(Self)` if no request was made, leaving the state unchanged
    fn poll_start_io(self, cx: &mut Self::Context) -> Result<Self::IOStartPending, Self>;
}

/// Typestate representing a server whose IO start request is pending.
///
/// Provides methods to handle the server’s response to the start request.
pub trait IOStartPendingContext: Sized {
    /// The parent client context associated with this state.
    type Context: ClientContext;

    /// The typestate representing an active IO session.
    type IOActive: IOActiveContext<Context = Self::Context>;

    /// Called when the server acknowledges the start request successfully.
    ///
    /// Returns the `Active` IO typestate.
    fn start_io(self, cx: &mut Self::Context) -> Self::IOActive;

    /// Called when the server permanently refuses the start request.
    ///
    /// Returns to the `Inactive` state, allowing the client to retry later.
    fn start_io_refused(self, cx: &mut Self::Context) -> <Self::Context as ClientContext>::IOInactive;

    /// Called when the server reports a temporary failure to start IO.
    ///
    /// The implementation may perform retries or log diagnostics. The current state
    /// remains in `StartPending`.
    fn start_io_failed(&mut self, cx: &mut Self::Context);
}

/// Typestate representing a server with active IO.
///
/// Provides methods to handle audio messages and to poll for stop requests.
pub trait IOActiveContext: Sized {
    /// The parent client context associated with this state.
    type Context;

    /// The typestate representing a pending IO stop request.
    type IOStopPending: IOStopPendingConxtext<Context = Self::Context>;

    /// Called when an audio message is received from the server.
    ///
    /// - `timestamp` is the time the packet was received
    /// - `header` is the audio message header
    /// - `data` is the raw audio payload
    ///
    /// The application can process, store, or forward the audio as needed.
    fn on_audio(
        &mut self,
        cx: &mut Self::Context,
        timestamp: std::time::Instant,
        header: syfala_proto::AudioMessageHeader,
        data: &[u8],
    );

    /// Polls whether the application requests stopping the active IO.
    ///
    /// - Returns `Ok(IOStopPending)` if a stop request was made
    /// - Returns `Err(Self)` if no stop request was made, leaving the state unchanged
    fn poll_stop_io(self, cx: &mut Self::Context) -> Result<Self::IOStopPending, Self>;
}

/// Typestate representing a server whose IO stop request is pending.
///
/// Provides methods to handle the server’s response to the stop request.
pub trait IOStopPendingConxtext: Sized {
    /// The parent client context associated with this state.
    type Context: ClientContext;

    /// Called when the server acknowledges the stop request successfully.
    ///
    /// Returns to the `Inactive` state.
    fn stop_io(self, cx: &mut Self::Context) -> <Self::Context as ClientContext>::IOInactive;

    /// Called when the server permanently refuses the stop request.
    ///
    /// Returns to the `Active` state, leaving IO running.
    fn stop_io_refused(self, cx: &mut Self::Context) -> Active<Self::Context>;

    /// Called when the server reports a temporary failure to stop IO.
    ///
    /// The implementation may perform retries or log diagnostics. The current state
    /// remains in `StopPending`.
    fn stop_io_failed(&mut self, cx: &mut Self::Context);
}


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
    Inactive(Inactive<Cx>),
    PendingStart(StartPending<Cx>),
    Active(Active<Cx>),
    PendingStop(StopPending<Cx>),
}

impl<Cx: ClientContext + ?Sized> ServerIOState<Cx> {
    /// Handles an incoming `Server::Connected` message.
    ///
    /// Dispatches control-plane and audio messages to the current state object,
    /// performs state transitions and callbacks where needed.
    ///
    /// Invalid messages for the current state are ignored, but may be logged.
    fn on_msg(
        &mut self,
        addr: core::net::SocketAddr,
        cx: &mut Cx,
        sock: &super::ClientSocket<impl crate::UdpSock>,
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
/// This type owns:
/// - The client context implementing connection and IO callbacks
/// - A priority queue of per-server deadlines
/// - A map of per-server IO state machines
///
/// It implements the `Client` so that it can be driven by a blocking UDP receive loop.
pub struct GenericClient<C: ClientContext> {
    /// User-provided callbacks defining connection, IO, and audio behavior.
    callbacks: C,
    /// Priority queue tracking next timeout per server.
    deadlines: ServerPQ<cmp::Reverse<std::time::Instant>>,
    /// Per-server state machine storage.
    servers: ServerMap<ServerIOState<C>>,
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
        }
    }

    /// Handles an incoming server connection request.
    ///
    /// If the server is not already connected, invokes `connect` on the
    /// client context to determine whether the connection is accepted.
    /// Sends a `Client::ConnectionResult` back to the server accordingly.
    fn on_server_connect_request(
        &mut self,
        sock: &super::ClientSocket<impl crate::UdpSock>,
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
        sock: &super::ClientSocket<impl crate::UdpSock>,
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
            self.deadlines.push(
                addr,
                cmp::Reverse(timestamp.checked_add(CONN_TIMEOUT).unwrap()),
            );
        }

        Ok(())
    }
}

// NIGHTLY: #[feature(map_try_insert)] use that where possible

impl<C: ClientContext> super::Client for GenericClient<C> {
    /// Handles an incoming UDP message (or lack thereof).
    ///
    /// If `maybe_msg` is `Some`, it is dispatched to protocol handlers.
    /// If `None`, a missing packet may be used later for logging or diagnostics.
    fn on_message(
        &mut self,
        sock: &super::ClientSocket<impl crate::UdpSock>,
        addr: core::net::SocketAddr,
        timestamp: std::time::Instant,
        maybe_msg: Option<(syfala_proto::message::Server, &[u8])>,
    ) -> std::io::Result<()> {
        match maybe_msg {
            Some(msg) => self.on_decoded_message(sock, addr, timestamp, msg)?,
            None => {
                // TODO: maybe an on_unknown_message cb?
            }
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
        sock: &super::ClientSocket<impl crate::UdpSock>,
    ) -> std::io::Result<()> {
        let now = std::time::Instant::now();

        // Expire all overdue servers
        while let Some((addr, _)) = self
            .deadlines
            .pop_if(|_, cmp::Reverse(deadline)| *deadline <= now)
        {
            self.servers.remove(&addr).unwrap();
        }

        sock.set_recv_timeout(self.deadlines.peek().map(|(_, cmp::Reverse(next))| {
            next.checked_duration_since(now)
                .unwrap_or(core::time::Duration::ZERO)
        }))?;
        Ok(())
    }
}
