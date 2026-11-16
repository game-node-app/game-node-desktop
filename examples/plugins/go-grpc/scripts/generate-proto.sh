#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
MODULE_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(CDPATH= cd -- "$MODULE_DIR/../../.." && pwd)
PROTO_DIR="$REPO_ROOT/crates/plugin-protocol/proto"
PROTOC_VERSION=36.1
GO_PROTO_VERSION=1.36.10
GO_GRPC_VERSION=1.5.1
GO_IMPORT=example.com/gamenode/plugins/go-grpc/proto

actual_protoc=$(protoc --version)
if [ "$actual_protoc" != "libprotoc $PROTOC_VERSION" ]; then
	printf 'expected protoc libprotoc %s, found %s\n' "$PROTOC_VERSION" "$actual_protoc" >&2
	exit 1
fi

go install "google.golang.org/protobuf/cmd/protoc-gen-go@v$GO_PROTO_VERSION"
go install "google.golang.org/grpc/cmd/protoc-gen-go-grpc@v$GO_GRPC_VERSION"

go_bin=${GOBIN:-$(go env GOPATH)/bin}
PATH="$go_bin:$PATH"
export PATH

actual_go_proto=$(protoc-gen-go --version)
if [ "$actual_go_proto" != "protoc-gen-go v$GO_PROTO_VERSION" ]; then
	printf 'expected protoc-gen-go v%s, found %s\n' "$GO_PROTO_VERSION" "$actual_go_proto" >&2
	exit 1
fi
actual_go_grpc=$(protoc-gen-go-grpc --version)
if [ "$actual_go_grpc" != "protoc-gen-go-grpc $GO_GRPC_VERSION" ]; then
	printf 'expected protoc-gen-go-grpc %s, found %s\n' "$GO_GRPC_VERSION" "$actual_go_grpc" >&2
	exit 1
fi

proto_include=${PROTO_INCLUDE_DIR:-/usr/include}
if [ ! -f "$proto_include/google/protobuf/struct.proto" ]; then
	printf 'set PROTO_INCLUDE_DIR to a directory containing google/protobuf/struct.proto\n' >&2
	exit 1
fi

protoc \
	-I "$PROTO_DIR" \
	-I "$proto_include" \
	--go_out="$MODULE_DIR/proto" \
	--go_opt=paths=source_relative \
	--go_opt="Mplugin.proto=$GO_IMPORT" \
	--go-grpc_out="$MODULE_DIR/proto" \
	--go-grpc_opt=paths=source_relative \
	--go-grpc_opt="Mplugin.proto=$GO_IMPORT" \
	plugin.proto
