# Syfala Protocol

A simple network protocol for audio endpoint discovery, and communication of packed, uncompressed, interleaved audio streams.

Requires the latest stable version of the [Rust](https://rust-lang.org/tools/install/) compiler.

Currently supports UDP for discovery, audio streaming, and control messages.

Multiple implementations of this protocol, for different backends, can be found in this repository.

## Crates

 - [`syfala_utils`](syfala_utils) A bunch of small utilities used throughout the rest of the crates in this repository.
 - [`syfala_proto`](syfala_proto) Type-based definitions of the protocol and it's communication model.
 - [`syfala_network`](syfala_network) Basic types implementing the protocol for different network protocols (currently only UDP).

### Backends

 - [`syfala_jack`](syfala_jack) For the JACK backend.
 - [`syfala_coreaudio`](syfala_coreaudio) For the coreaudio backend

Beware of clock drift, an issue unaddressed in all of the backends.

You can run:

```shell
cargo doc --open -p <package_name>
```

(replacing <package_name> with one of the crates listed above)

To view the documentation for any of the crates above.