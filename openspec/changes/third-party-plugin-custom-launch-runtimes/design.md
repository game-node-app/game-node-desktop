# Design

## Boundary and dependencies

This follow-up builds on the provider/catalog MVP and the user-intent/configuration change. Existing partial task 7.8 work is preserved in the shared checkout and remains uncommitted; it is owned here. Do not implement or commit it as part of the MVP.

Provider-free extension process management is part of this follow-up. The MVP may review and store a signed extension package in a disabled state, but cannot activate it because its production supervised runtime is provider-only. The follow-up adds an extension-kind process slot and account-bound lifecycle without fabricating a provider id: each extension session is authenticated as the plugin and scoped to the user-selected target game. Provider-only RPCs remain unavailable. Inventory can offer activation only when the host has compatible extension-runtime support and a valid target context.

## Launch recipes and review

Resolve a typed immutable plan from the detected target plus approved recipe/runtime/mod components. Bind approvals to canonical plan content and package, source, target, resource and grant revisions. Revalidate before dispatch; changed or revoked inputs require a new user decision. Keep the detected default as an explicit choice and never downgrade a denied custom request into a standard launch.

## Runtime and content identity

Register runtimes as approved host resources. Bind content and installations to the existing GameNode game identity so multiple runtimes do not create duplicate games or transfer save/history ownership. Runtime-session reports are typed, scoped, time-bounded evidence; Core arbitrates duplicates, stale reports and PID reuse and retains existing pause/accounting controls.
