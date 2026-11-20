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
