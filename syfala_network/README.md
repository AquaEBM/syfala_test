# `syfala_network`

Implementation of the message model defined in the `syfala_proto` crate.

This crate provides the runtime machinery needed to send and receive protocol
messages over network sockets, along with lightweight abstractions for client and
server state management.

## Scope

- Encoding and decoding protocol messages using `serde` and `postcard`.
- Transport over network sockets (currently, UDP only)
- Small helper traits for driving client and server states

This crate is intentionally transport-focused: it does not redefine or modify the
protocol itself, but instead implements a concrete wire representation and
communication layer for the message model described in `syfala_proto`.
