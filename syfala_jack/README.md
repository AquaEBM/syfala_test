# `syfala_jack` (incomplete)

Implements a JACK Client and server using [`syfala_network`](../syfala_network)

Refer to the documentation of [`syfala_network`](../syfala_network)

This implementation requires an active JACK server to launch

This implementation is quite simple:

When a connection is requested. It is necessary to:

- check if all of the streams' sample rates are exactly equal to that of the active JACK server
- check if all of the streams' sample formats are `IEEF32`, because that is the only sample format
JACK supports.

When the above conditions are met, IO start is immediately requested (`IOInactiveContext::poll_start_io`
returns ok immediately) and, upon a successful response from the server, a JACK client that has

- As many inputs as the server's outputs
- As many outputs as the server's inputs

Is launched, that sends incoming audio data to an audio sender thread
and receives incoming audio data from the running network thread (the UDP receive loop
implemented in `syfala_proto`). All, using the utilities provided in
[`syfala_utils`](../syfala_utils/), i.e. adapters around lock-free, wait-free FIFO SPSC ring
buffers.