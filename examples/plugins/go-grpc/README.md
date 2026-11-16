# Go gRPC provider example

This is a standalone third-party provider written in Go. It does not import the GameNode Rust SDK
or any workspace package. The committed protobuf and gRPC bindings implement the public protocol
from `crates/plugin-protocol/proto/plugin.proto`.

The provider negotiates API 1.6, authenticates with the host-issued session credential, reads its
host context, and exchanges discovery, catalog and counter results over `ProviderHost.Exchange`.
Its sample records are fictional. The integration test runs the same wire fixture over a Linux Unix
socket and, on Windows, a current-user named pipe.

## Build and run

From this directory:

```sh
go test ./...
go build ./cmd/provider
```

The executable reads `GAMENODE_PLUGIN_ENDPOINT`, `GAMENODE_PLUGIN_SESSION_CREDENTIAL` (64
hexadecimal characters for the 32-byte session credential), and `GAMENODE_PLUGIN_OPERATION_ID`. These values
are supplied by a host launcher; never put a real credential in source control or logs. The host
must grant `provider.context.read`, `library.discovery`, `library.catalog`, and `playtime.counter`.

## Regenerate protocol bindings

Generation reads the canonical workspace proto, but the checked-in generated code lets this module
build without a workspace checkout. Install `protoc` **libprotoc 36.1** and run:

```sh
PROTO_INCLUDE_DIR=/path/to/protoc/include ./scripts/generate-proto.sh
```

The script installs pinned `protoc-gen-go v1.36.10` and `protoc-gen-go-grpc v1.5.1`, checks all
tool versions, and writes only to `proto/`. `PROTO_INCLUDE_DIR` must contain
`google/protobuf/struct.proto`; on Linux `/usr/include` is used when the variable is unset.

## Transports

Linux uses a local Unix-domain socket. Windows uses a named pipe and the pinned `go-winio` module.
The transport adapters only create the local `net.Conn`; gRPC handles framing and protobuf in the
same way on both operating systems. Windows pipe execution must be verified on a native Windows
host; a Linux cross-compile checks that its adapter and fixture compile.
