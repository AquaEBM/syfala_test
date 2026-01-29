# `syfala_network`

Implementation of the message model defined in the `syfala_proto` crate.

This crate provides the runtime machinery needed to send and receive protocol
messages over network sockets, along with lightweight abstractions for client and
server state management.

## Scope

- Encoding and decoding protocol messages using `serde` and `postcard`.
- Transport over network sockets (currently, UDP only)
- Small helper traits for driving client and server states
- Optionally a pluggable, callback based implementation of a client (TODO: server)

This crate is intentionally transport-focused: it does not redefine or modify the
protocol itself, but instead implements a concrete wire representation and
communication layer for the message model described in `syfala_proto`.

## `#[feature = "generic"]`

By enabling this feature, the module also exposes a generic implementation of a client (TOOD: server)
driven by a fully blocking UDP receive loop, that stores server state in a hashmap, and that
stores connection timeout deadlines in a priority queue.

This implementation is simply a callback handler that calls callbacks provided by the user
to perform various tasks like

- check if the application requests to start/stop IO
- forward audio data to the application
- run extra code when new connections are established, or when IO starts, or stops
- ...

### Design

This implementation uses typestate logic to represent different states a remote server may be in:

```
IOInactive <-> IOStartPending -> IOActive <-> IOStopPending ( -> back to IOInactive)
```

Each state is represented by a different type and transitions are performed by using callbacks
that consume instances of the previous state.