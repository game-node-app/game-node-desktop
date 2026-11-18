# Design

## Boundary and dependencies

This follow-up builds on the provider/catalog MVP and the user-intent/configuration change. Task 7.8 implementation is tracked in the runtime worktree; its integration and validation remain owned by this follow-up.

Provider-free extension process management is part of this follow-up. The MVP may review and store a signed extension package in a disabled state, but cannot activate it because its production supervised runtime is provider-only. The follow-up adds an extension-kind process slot and account-bound lifecycle without fabricating a provider id: each extension session is authenticated as the plugin and scoped to the user-selected target game. Provider-only RPCs remain unavailable. Inventory can offer activation only when the host has compatible extension-runtime support and a valid target context.

## Launch recipes and review

Resolve a typed immutable plan from the detected target plus approved recipe/runtime/mod components. Bind approvals to canonical plan content and package, source, target, resource and grant revisions. Revalidate before dispatch; changed or revoked inputs require a new user decision. Keep the detected default as an explicit choice and never downgrade a denied custom request into a standard launch.

The shared host review is a prerequisite for user selection and Play dispatch. Implement Source 10.7 before Source 10.6 so both plugin-initiated and user-initiated launches present the same host-resolved plan. Store one active recipe selection per library entry/profile; a missing selection means the detected default. A removed, disabled or revised recipe remains an explicit unavailable selection until the user chooses another recipe or returns to the detected default. Reuse remembered consent only for the same canonical plan and current package, source, target, resource and grant revisions.

## Runtime and content identity

Register runtimes as approved host resources. Bind content and installations to the existing GameNode game identity so multiple runtimes do not create duplicate games or transfer save/history ownership. Runtime-session reports are typed, scoped, time-bounded evidence; Core arbitrates duplicates, stale reports and PID reuse and retains existing pause/accounting controls.

## Runtime transitions and report freshness

A runtime instance and PID can remain active while its game changes. The host validates an independently authorized game/installation context and current profile and grant revisions for the next game before attribution; a plugin may translate an emulator control for selecting the next game within its authorized scope. Close the preceding confirmed interval at the validated boundary; absent, denied, stale or ambiguous evidence produces an unknown gap, not attribution to either game. Do not require restarting the runtime or widen session authority. Correlate PID reuse conservatively. Initially expire reports after 30 seconds using host monotonic receipt time; recommend reporting at least every 10 seconds and document timestamp acceptance before delivery, and test delayed reports and expiry. Restart requires fresh confirmation.
