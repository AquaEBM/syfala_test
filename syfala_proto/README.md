# `syfala_proto`

This crate is where most of the protocol is defined and explained. We only use abstract types. Encoding and decoding is up to the user.

The types in this crate already implement `serde`'s `Serialize` and `Deserialize` traits, for the user to conveniently plug into other `serde` backends.