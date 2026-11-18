# Go protocol interoperability and configuration example

The Go fixture uses generated API protobuf messages directly, without a GameNode SDK or host-side
dynamic RPC dispatcher. It checks exact revisions/timestamps, preservation of unknown extension
namespaces, typed runtime platform/argv fields, and stable content IDs. Launch-consent fixtures cover
emulator/runtime, known mod-layer and environment-only recipes, keep execution bound to the reviewed
recipe reference, and check that installation and file writes use separate ResourceHost requests.
The `cmd/configuration` provider is a minimal raw-gRPC client: it negotiates API 1.6, exchanges
provider commands, opens/focuses/closes a native preference window, and saves its non-secret
preference to host-owned persistent storage.

Run it with:

```sh
go test ./...
```

Build the plugin executable for the native host OS with `go build -o <package-entrypoint>
./cmd/configuration`. The Windows build uses the private named pipe transport; Linux uses the
supervisor's Unix socket. See [`../plugins/README.md`](../plugins/README.md) for native dialog
dependencies, permission grants and the concurrent-provider walkthrough.

The message bindings are generated from `crates/plugin-protocol/proto/plugin.proto` with
`protoc-gen-go`; regenerate them when the public wire contract changes. This fixture covers wire
compatibility only. It checks the declared ResourceHost wire contract and does not claim that every
method is served by the desktop.
