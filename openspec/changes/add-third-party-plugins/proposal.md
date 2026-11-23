# Third-party Plugin Provider and Catalog MVP

## Why

The full third-party plugin proposal combines a usable provider/catalog foundation with optional game actions, custom launch, shared data and telemetry. Shipping these as one change keeps useful provider endpoints behind unrelated long-running work. This first delivery slice makes trusted provider plugins installable and manageable while preserving account isolation and official plugin behavior.

## What Changes

- Completes external provider composition and account-bound plugin session lifecycle.
- Delivers package import, plugin activation controls, publisher and effective-permission review, and safe update recovery.
- Keeps every lifecycle operation live: import, enable, disable, update/rollback, uninstall and reinstall take effect in the running desktop, including the library store list and rows, without a restart.
- Delivers provider discovery/catalog/counter interoperability, SDK distribution and focused Rust examples.
- Validates Linux and Windows provider-session paths without enabling the deferred endpoint families.

## Out of Scope

Game launch/install requests and folder approvals move to `third-party-plugin-actions-and-configuration`. Custom launch, runtimes and runtime-session accounting move to `third-party-plugin-custom-launch-runtimes`. Shared datasets, diagnostics, telemetry, persistent storage and cleanup move to `third-party-plugin-data-and-persistence`. Their contracts remain described in the existing design/spec deltas as the broader program reference; the follow-up task lists own implementation progress. The in-progress local implementation of task 7.8 is preserved and tracked by its follow-up change.
