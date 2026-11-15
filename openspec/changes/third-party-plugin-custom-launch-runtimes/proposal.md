# Third Party Plugin Custom Launch Runtimes

## Why

Custom launch recipes, runtime bindings and plugin-reported game sessions affect executable selection, attribution and playtime. Isolating this work after the provider/catalog MVP gives launch behavior its own consent, revision and compatibility boundary.

## What Changes

Adds custom executable/URI recipes and overlays, immutable reviewed launch plans, runtime resources/content identity and runtime-session accounting. It also adds launch-specific UI and Go/Rust interoperability coverage.

## Out of Scope

Does not modify official provider implementations, launch defaults, credentials or accounting policy. MVP providers cannot customize launch or register runtimes.
