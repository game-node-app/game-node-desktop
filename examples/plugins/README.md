# Plugin-owned configuration examples

The Rust SDK and Go examples implement the same small provider: a native window edits the
non-secret `preferred-catalog-label` value through the host's persistent plugin storage. They
request `ui.configure`, `storage.read`, `storage.write` and `storage.persist`; no preference value
is stored in a file owned by the plugin.

## Build and fixture checks

Run from the repository root:

```sh
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo check --manifest-path examples/plugins/rust-sdk/Cargo.toml
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo test --manifest-path examples/plugins/rust-sdk/Cargo.toml
go -C examples/go test ./...
```

For a Linux native build, install `zenity` and `wmctrl`; for Windows, use PowerShell with
Windows Forms available. Include the files in `shared/` beside each packaged plugin under
`plugins/shared/`. The dialog runs in a child process owned by the plugin. Its output is saved
through `StoragePut` with persistent durability when the user saves and closes it.

## Native acceptance walkthrough

Build the Rust plugin and the Go plugin natively for the machine running GameNode. Package each
binary with its `plugin.json`, the matching platform entrypoint, `shared/config-window.sh` and
`shared/config-window.ps1`; sign the package with a local development key as described in
[`docs/plugin-package-manifest.md`](../../docs/plugin-package-manifest.md#building-and-checking-a-package).
At import, grant `ui.configure`, `storage.read`, `storage.write` and `storage.persist` to the
example. The persistent-storage grant is required for the preference to survive a plugin restart.

After enabling the plugin, open its Configure window and leave it visible. While it remains open,
trigger a provider refresh or counter read from GameNode. The example returns the Open result as
soon as the child window starts, so the provider stream can continue servicing those operations.
Choose Configure again to focus the existing window, save a new label, close it, and reopen it to
confirm the stored value survived. A host close request stops the owned window process; profile
changes start a fresh plugin session with its own host storage context.

The host configuration lifecycle from task 1.1 is implemented in this checkout. The full native
walkthrough still depends on third-party process supervision (source task 8.3) and the permission
inventory/approval UI (source tasks 10.1 and 10.2). Until those land, the fixture tests cover
lifecycle correlation and concurrent provider dispatch, while the package build commands only
compile the examples.
