# Design

## Boundary and dependencies

This follow-up builds on the provider/catalog MVP and the user-intent/configuration change. Existing partial task 7.8 work is preserved in the shared checkout and remains uncommitted; it is owned here. Do not implement or commit it as part of the MVP.

## Launch recipes and review

Resolve a typed immutable plan from the detected target plus approved recipe/runtime/mod components. Bind approvals to canonical plan content and package, source, target, resource and grant revisions. Revalidate before dispatch; changed or revoked inputs require a new user decision. Keep the detected default as an explicit choice and never downgrade a denied custom request into a standard launch.

The shared host review is a prerequisite for user selection and Play dispatch. Implement Source 10.7 before Source 10.6 so both plugin-initiated and user-initiated launches present the same host-resolved plan. Store one active recipe selection per library entry/profile; a missing selection means the detected default. A removed, disabled or revised recipe remains an explicit unavailable selection until the user chooses another recipe or returns to the detected default. Reuse remembered consent only for the same canonical plan and current package, source, target, resource and grant revisions.

## Runtime and content identity

Register runtimes as approved host resources. Bind content and installations to the existing GameNode game identity so multiple runtimes do not create duplicate games or transfer save/history ownership. Runtime-session reports are typed, scoped, time-bounded evidence; Core arbitrates duplicates, stale reports and PID reuse and retains existing pause/accounting controls.
