# Go protocol interoperability and configuration example

The wire fixture uses generated API protobuf messages directly, without a GameNode SDK. It checks
exact revision/timestamp values and preservation of unknown extension namespaces. The
`cmd/configuration` provider is a minimal raw-gRPC client: it negotiates API 1.6, exchanges provider
commands, opens/focuses/closes a native preference window, and saves its non-secret preference to
host-owned persistent storage.

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
compatibility only. It does not claim that every `ResourceHost` method is served by the desktop; only the Library reads are.
