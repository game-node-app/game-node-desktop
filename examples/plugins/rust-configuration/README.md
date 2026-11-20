# Rust configuration provider

This example uses the public Rust plugin SDK. It opens one native preference window at a time,
focuses the existing process for repeated Configure requests, handles Close, and stores the label
through the plugin's host-owned persistent storage namespace.

From the repository root, build and run fixture checks with:

```sh
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo check --manifest-path examples/plugins/rust-configuration/Cargo.toml
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo test --manifest-path examples/plugins/rust-configuration/Cargo.toml
```

Linux uses `zenity` for the window and `wmctrl` to focus it. Windows uses the bundled PowerShell
Windows Forms helpers. See [`../README.md`](../README.md) for permissions, packaging and the
native walkthrough/dependencies.

## Native run: configuration while the provider keeps working

1. Build the binary, package it with `shared/config-window.sh` and `shared/config-window.ps1`
   under `plugins/shared/`, and sign it as described in
   [`../README.md`](../README.md#package-and-sign) and
   [`docs/plugin-package-manifest.md`](../../../docs/plugin-package-manifest.md#building-and-checking-a-package).
2. Import the package, grant `ui.configure`, `storage.read`, `storage.write` and
   `storage.persist`, and enable the plugin.
3. Choose **Configure**. The plugin starts one native window in a child process and returns the
   Open result immediately, so the provider stream stays free.
4. While the window is still open, request a provider operation (for example a library refresh or
   a counter read) from GameNode. It completes normally: the window and the provider session are
   separate, and the plugin never blocks its event loop on the window.
5. Choose **Configure** again: the existing window is focused rather than duplicated. Change the
   label, save and close, then reopen. The label is read back from host-owned persistent storage
   and survives a plugin restart only with the persistent-storage grant.
6. A host close request stops the window process. Switch account profiles and open Configure
   again: the new session gets its own storage context and can choose a different label.

The command-line fixture `fixture_configure_focus_close_and_persist_preference_lifecycle` covers
the same lifecycle without a display. Native window execution on Windows remains part of 18.3.
