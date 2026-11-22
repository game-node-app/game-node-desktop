# Third Party Plugin Actions And Configuration

## Why

MVP establishes trusted plugin packaging, provider composition, signed-in account-bound sessions, catalog operations, explicit permissions, activation controls and recovery. The next slice adds user-mediated game actions and plugin-owned configuration without expanding the authority of the provider/catalog MVP.

## What Changes

Adds confirmed launch/install intents, scoped folder approvals, optional plugin configuration windows/forms, and their example coverage. A user may start an enabled third-party extension from a game's Core actions; Core starts a verified, one-shot extension session scoped to that selected game and installation and supplies an intent for the selected Play or Install action. Depends on the provider session, permission inventory and process lifecycle delivered by the MVP.

## Out of Scope

Does not change official provider behavior or allow plugins to bypass host confirmation. Filesystem scope and game actions remain unavailable until the corresponding approvals are implemented.
