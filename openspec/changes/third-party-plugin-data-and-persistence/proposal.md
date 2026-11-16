# Third Party Plugin Data And Persistence

## Why

Cross-plugin data, diagnostics and durable storage need separate producer/recipient consent, provenance, quotas, retention and cleanup controls. Delivering them as a later change keeps the MVP catalog path private and read-focused.

## What Changes

Adds typed shared data and projections, diagnostic/gameplay telemetry, durable plugin-owned persistence, cleanup administration, consent UI, two-client interoperability coverage, and explicit downgrade/publisher-key re-trust review with a rollback-safe private-data policy.

## Out of Scope

Does not make any dataset public by default, add a Core collector/dashboard, expose physical persistence paths or let a grant reveal another plugin’s private namespace. Package-version changes do not create new owner namespaces or automatically migrate/delete older schema-versioned data.
