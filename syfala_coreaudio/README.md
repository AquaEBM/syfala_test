# `syfala_coreaudio`

A (work in progress) implementation of the protocol, for use with CoreAudio.

Uses the [`AudioServerPlugin`](https://developer.apple.com/documentation/coreaudio/creating-an-audio-server-driver-plug-in) API.

## Useful Commands

### Stream logs:

```shell
log stream --level debug --predicate 'subsystem == "com.emeraude.syfala_coreaudio"'
```

### Build library:

```shell
cargo build -r
```

### Install driver bundle:

First, open [`scripts/install.sh`](scripts/install.sh) and write your Apple Developer ID inside
the quotation marks after TEAM_ID.

Then, run:

```shell
./scripts/install.sh ../target/release/libsyfala_coreaudio.dylib
```