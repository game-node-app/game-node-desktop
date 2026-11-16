# Go protocol interoperability fixture

This small fixture uses the generated API protobuf messages directly, without a GameNode SDK or
host-side dynamic RPC dispatcher. `extension_data.go` builds a hybrid typed `RuntimeRecord`, keeps
an unknown versioned namespace unchanged during a local patch, demonstrates consumer-side field
selection, and constructs custom dataset and opaque-byte wire shapes.

Run it with:

```sh
go test ./...
```

The message bindings are generated from `crates/plugin-protocol/proto/plugin.proto` with
`protoc-gen-go`; regenerate them when the public wire contract changes. This fixture covers wire
compatibility and local examples only. Private `StoragePut` carries opaque bytes; those bytes are
not cross-plugin data. `DataPublish` and the runtime update/extension patch operation are currently
unserved by the desktop host, so the custom dataset and hybrid patch examples construct messages
but do not invoke working RPCs. A production consumer must rely on host-enforced authorization and
projection; a local field filter does not grant access. Typed identity, ownership, handles,
revision, and observation time are interpreted by Core. Extension `Struct` values are declarative
metadata only: Core does not infer identity, grants, executable behavior, or paths from them.
