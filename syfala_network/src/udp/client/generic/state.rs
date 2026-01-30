
/// Type alias representing the `Inactive` IO state for a given client context.
///
/// Resolves to the associated `IOInactive` type of the `ClientContext`.
pub type Inactive<Cx> = <Cx as ClientContext>::IOInactive;

/// Type alias representing the `StartPending` IO state for a given client context.
///
/// Resolves to the associated `IOStartPending` type of the `Inactive` state.
pub type StartPending<Cx> = <Inactive<Cx> as IOInactiveContext>::IOStartPending;

/// Type alias representing the `Active` IO state for a given client context.
///
/// Resolves to the associated `IOActive` type of the `StartPending` state.
pub type Active<Cx> = <StartPending<Cx> as IOStartPendingContext>::IOActive;

/// Type alias representing the `StopPending` IO state for a given client context.
///
/// Resolves to the associated `IOStopPending` type of the `Active` state.
pub type StopPending<Cx> = <Active<Cx> as IOActiveContext>::IOStopPending;

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

    fn unknown_message(
        &mut self,
        addr: core::net::SocketAddr,
    );
}

/// "Typestate" representing a server with inactive IO.
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

/// "Typestate" representing a server whose IO start request is pending.
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

/// "Typestate" representing a server with active IO.
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

/// "Typestate" representing a server whose IO stop request is pending.
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