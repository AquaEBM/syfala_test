# Useful Commands

Stream logs:

```shell
log stream --level debug --predicate 'subsystem == "com.emeraude.syfala_coreaudio"'
```

Build library:

```shell
cargo build -r
```

Install driver bundle:
```shell
./scripts/install.sh ../target/release/libsyfala_coreaudio.dylib
```