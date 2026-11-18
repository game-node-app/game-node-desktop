# Go protocol interoperability fixture

This small fixture uses the generated API protobuf messages directly, without a GameNode SDK or
host-side dynamic RPC dispatcher. It checks that typed runtime platform/argv fields and stable
content IDs round-trip, that exact 64-bit revisions/timestamps remain intact, and that a consumer
changing one known extension namespace preserves an unselected namespace unchanged at the
protobuf message level. Launch-consent fixtures cover emulator/runtime, known mod-layer and
environment-only recipes, keep execution bound to the reviewed recipe reference, and check that
installation and file writes use separate ResourceHost requests.

Run it with:

```sh
go test ./...
```

The message bindings are generated from `crates/plugin-protocol/proto/plugin.proto` with
`protoc-gen-go`; regenerate them when the public wire contract changes. This fixture covers wire
compatibility only. It checks the declared ResourceHost wire contract and does not claim that every
method is served by the desktop.
