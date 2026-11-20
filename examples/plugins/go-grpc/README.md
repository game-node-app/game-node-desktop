# Go gRPC provider example

This is a standalone third-party provider written in Go. It does not import the GameNode Rust SDK
or any workspace package. The committed protobuf and gRPC bindings implement the public protocol
from `crates/plugin-protocol/proto/plugin.proto`.

The provider negotiates API 1.6, authenticates with the host-issued session credential, reads its
host context, and exchanges discovery, catalog and counter results over `ProviderHost.Exchange`.
Its sample records are fictional. Configure opens or focuses one plugin-owned native preference
window per session and stores the non-secret catalog label through host-owned persistent storage.
The integration fixture covers window correlation, preference persistence and reopening. The same
fixture also runs over a Linux Unix socket and, on Windows, a current-user named pipe.

## Build and run

From this directory:

```sh
go test ./...
go build ./cmd/provider
```

The executable reads `GAMENODE_PLUGIN_ENDPOINT`, `GAMENODE_PLUGIN_SESSION_CREDENTIAL` (64
hexadecimal characters for the 32-byte session credential), and `GAMENODE_PLUGIN_OPERATION_ID`. These values
are supplied by a host launcher; never put a real credential in source control or logs. The host
must grant `provider.context.read`, `library.discovery`, `library.catalog`, `playtime.counter`,
`ui.configure`, `storage.read`, `storage.write`, and `storage.persist`. The persistent-storage grant
is required for the preference to survive a plugin restart. Linux configuration uses `zenity` and
`wmctrl`; Windows uses the bundled PowerShell Forms and focus helpers in `../shared/`.

## Package, sign and install

`package/` is the pack source: the manifest, the MIT license and this example's third-party
notices. It declares one Linux x86-64 target; the host selects the entrypoint from the signed
target tuple. The manifest requests exactly the eight capabilities the example negotiates, so the
host can grant the configuration and persistent-storage scopes its Configure window uses.

1. Build a static binary and place it at the declared entrypoint:

   ```sh
   CGO_ENABLED=0 go build -trimpath -o package/bin/linux-x64/plugin ./cmd/provider
   ```

   To package Windows instead, change the `targets` entry in `package/plugin.json` to
   `{"os":"windows","arch":"x86_64","abi":"msvc","entrypoint":"bin/windows-x64/plugin.exe"}`,
   build with
   `GOOS=windows GOARCH=amd64 go build -trimpath -o package/bin/windows-x64/plugin.exe ./cmd/provider`,
   and keep the Windows transport test evidence separate. A target whose entrypoint is missing
   from the signed payload is rejected at import.

   `package/shared/` carries the native window helpers inside the signed payload. The plugin looks
   beside its installed entrypoint for them, so an installed package needs no host-wide copy.

2. Generate a development-only signing key and note the printed public key:

   ```sh
   python3 -m pip install -r scripts/plugin-package-requirements.txt
   python3 scripts/plugin-package.py keygen --development --private-key ./fixture-dev.pem
   ```

   Replace `publisher.public_key` in `package/plugin.json` with the printed value and choose any
   `key_id`. The committed placeholder key cannot sign a package: `pack` refuses a private key that
   does not match the manifest publisher key. A development key adds `DEVELOPMENT-KEY.txt` to the
   archive and is never a production identity; keep the private key out of the repository.

3. Pack and verify without executing anything:

   ```sh
   python3 scripts/plugin-package.py pack \
     --source examples/plugins/go-grpc/package \
     --private-key ./fixture-dev.pem \
     --output ./go-grpc.gnplugin
   python3 scripts/plugin-package.py verify ./go-grpc.gnplugin
   ```

4. In GameNode, import `go-grpc.gnplugin`. The review shows the untrusted publisher fingerprint,
   the MIT license and these notices; confirm that exact fingerprint to pin the publisher, then
   install the package. It stays disabled until you review permissions: grant
   `provider.context.read` and `library.discovery` and enable the plugin. The catalog, counter,
   `ui.configure` and storage scopes are optional; the Configure window only opens with
   `ui.configure`, and the label only survives a plugin restart with `storage.persist`.

5. After enabling, the fictional `example-go` provider reports its fixture games, catalog and
   counter. Configure opens or focuses the example's preference window over the bundled
   `shared/` helpers; the preference is saved through host-owned storage, not a plugin file. The
   example never starts a game.

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
