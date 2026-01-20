# `syfala_proto`

A simple, low-latency protocol for real-time audio communication and discovery.

This crate defines a message-based protocol intended for real-time audio
streaming.

### Roles

Each endpoint acts as either a **client** or a **server**:

- **Servers** are typically firmware running on external or embedded devices.
- **Clients** are typically drivers or applications running on consumer hardware.

## Model

The protocol is defined entirely in terms of typed messages exchanged between
endpoints. These messages fall into three broad categories:

- **Connection / discovery messages**
- **Control messages**
- **Audio messages**

See the rustdoc-generated docs for complete message definitions.

## Connection and discovery

Connection messages are used to establish communication between endpoints.
For example, when a server receives a `Client::Connect`
message from an unknown client, it may respond with a
`Server::Connect` message to accept the connection.

Once this exchange succeeds, a logical "connection" is established.

Servers advertise their supported stream formats as part of the connection
process. These formats are **fixed for the lifetime of the connection** and
define the audio formats used during active I/O.

If a client is incompatible with any advertised stream format, it must refuse
the connection.

Connection messages may be sent to broadcast addresses to support service
discovery on a local network.

## Control messages

Control messages are infrequent messages used to coordinate behavior between
connected endpoints. Currently, they are limited to requests to start or stop
audio I/O.

Clients may request that audio I/O be started. Upon receiving such a request, servers
must perform any required initialization, allocation, and clock anchoring **before**
replying with a success response.

A successful response indicates that the server is _immediately_ ready to send and
receive audio data.

If the server fails to start I/O, or explicitly refuses the request, it must report
the failure back to the client.

The same thing happens with Stopping IO, servers free the corresponding resources,
then report back.

## Audio messages

When a connection is established and I/O is active, endpoints exchange audio
messages.

Audio messages carry raw audio bytes along with stream indices and byte offsets
to allow receivers to interpet how to decode the data and handle packet loss and
reordering.

The types in this crate already implement `serde`'s `Serialize` and `Deserialize`
traits, for the user to conveniently plug into other `serde` backends.