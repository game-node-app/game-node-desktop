# Go protocol interoperability fixture

This small fixture uses the generated API protobuf messages directly, without a GameNode SDK or
host-side dynamic RPC dispatcher. It checks that a typed `RuntimeRecord` keeps exact 64-bit
revision/timestamp values and that a consumer changing one known extension namespace preserves an
unselected namespace unchanged at the protobuf message level.

Run it with:

```sh
go test ./...
```

The message bindings are generated from `crates/plugin-protocol/proto/plugin.proto` with
`protoc-gen-go`; regenerate them when the public wire contract changes. This fixture covers wire
compatibility only. It does not claim that the API 1.4 `ResourceHost` is mounted by the desktop.
