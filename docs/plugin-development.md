# Developing a GameNode provider plugin

GameNode's provider API lets a bundled provider process implement store-specific behavior while
the desktop application remains responsible for the library, profile, persistence and GameNode
account. The process communicates with the running desktop through the versioned public crates
`gamenode-plugin-protocol` and `gamenode-plugin-sdk`. Provider plugins must not depend on
`gamenode-core`, `gamenode-app`, `gamenode-api` or `gamenode-platform`; they must not read or write
the GameNode database or GameNode credentials.

## Session and operations

The host starts a registered plugin process and provides its local endpoint, a temporary session
credential and an operation identifier through the environment. The plugin opens a session with
`Session::open(Launch::from_env()?, plugin_id, provider_id, version, capabilities)` and implements
the async `ProviderPlugin` trait. `run(session, Arc::new(plugin))` dispatches host commands. Use the
`HostClient` scoped to each command for approved services, and return an `OperationResult` through
the SDK. Do not retain a command's client to authorize later work: operation, account and profile
generations fence stale requests.

The public API currently defines discovery, account, catalog, login, secret, counter and action
operations. A provider should request only the capabilities it actually needs. The host compares
the request with trusted packaged metadata and grants the intersection; a plugin cannot grant
itself additional access. API major/minor negotiation and unknown or duplicate permission checks
are implemented in `gamenode-plugin-protocol`.

## Capabilities

The complete capability identifiers currently defined by the public protocol are:

| Capability | What the host service permits |
| --- | --- |
| `provider.context.read` | Read this provider's validated configuration, platform and negotiated operation limits. It does not expose GameNode credentials or another provider's settings. |
| `library.discovery` | Submit installed-game observations for the current provider scan. The host reconciles them with the library; this is not direct library or database write access. |
| `library.catalog` | Submit complete owned and family-sharing catalog outcomes for the current account operation, including categorized failures. This is not access to the PC ports catalog or its installation workflow. |
| `account.state.read` | Read this provider's non-secret account identity and status. |
| `account.state.publish` | Publish validated account transitions and status. Secret values are excluded from account state and GUI snapshots. |
| `auth.webview` | Open, focus, cancel and close this provider's isolated sign-in attempt. The host enforces the packaged navigation policy. |
| `auth.webview.cookies.read` | Read only the approved cookie names for the active login attempt. |
| `auth.webview.page.read` | Read only the bounded page probe at the exact origin in trusted login policy. Plugins cannot submit arbitrary scripts or origins. |
| `secrets.read` | Read this provider's logical secret slot. |
| `secrets.write` | Write this provider's logical secret slot. |
| `secrets.delete` | Delete this provider's logical secret slot. |
| `playtime.counter` | Return provider-local total-playtime and last-played observations. Core retains tracking, attribution, deduplication and submission policy. |
| `provider.actions.declare` | Declare approved provider actions that the host validates against trusted URI schemes and templates. It does not authorize arbitrary commands, shells or filesystem changes. |

Secret read, write and delete are independent grants. The host uses Windows Credential Manager on
Windows and the existing Secret Service/private-file policy on Linux. For Steam, the logical
`session` slot is mapped by trusted host metadata to the existing physical `CredentialSlot::Steam`
alias (`steam` in the keyring and `steam-token` in the Linux private-file fallback). The GameNode
session remains in its separate `Session` slot. Windows has no plaintext fallback. A plugin must
not treat a reported storage method as proof that a secret has been persisted successfully.

The Steam plugin uses the full capability set above to provide installed discovery, account
sign-in/check/sign-out, owned and Family catalog sync, Steam playtime observations and Play/Install
actions. Its trusted descriptor also contains Steam-specific native-access declarations described
below. Optional settings capability `ui.settings` exists in the protocol crate, but is outside the
Steam integration scope and is not part of Steam's grant. There is no plugin-provided catalog
browsing UI or PC ports installation service in this integration.

## Host capabilities and native access are different

Host capabilities authorize calls to explicit GameNode services. They are scoped to the plugin,
provider, process session and operation, and the host checks the grant on each call. For example,
`secrets.read` authorizes a request for the plugin's logical secret; it does not authorize reading
arbitrary files or the operating system's credential database directly.

Native-access declarations describe what the trusted bundled executable needs to access for its
own implementation. Steam needs HTTPS access to Steam's store/API, reads of Steam library and
configuration files (including supported Linux native/Flatpak locations), and the Steam install
root from the current user's Windows registry. These declarations document expected access; they
are not operating-system sandbox rules. The plugin is an ordinary process running with the user's
OS permissions, so filesystem and network access by that process are not confined by the host API.
Community code is not enabled by these bundled-plugin permissions or declarations.

## Steam-specific implementation rules

- Keep Steam parsing, account/session validation, HTTP calls, login interpretation, discovery and
  Steam's own playtime counter inside the Steam plugin. Use the public host services for login
  windows, provider secrets, account state, discovery/catalog results and action declarations.
- Keep GameNode authentication, matching, library reconciliation, favorites, artwork, profile
  persistence, playtime attribution and submission policy in the desktop/Core.
- Steam login URLs, allowed hosts, cookies and page probe come from trusted packaged policy data.
  Runtime requests must not replace that policy. Do not log cookies, tokens, credentials, raw
  secret-bearing messages or complete account URLs.
- Distinguish a successful empty result from a failed or incomplete operation. A failure must not
  be reported as an empty catalog or discovery result; the host preserves prior successful facts
  according to the operation's contract.
- Keep blocking file, registry and credential access off the UI thread. Network work and user
  login waits must remain cancellable and must not block the desktop event loop.

The concrete DTOs, RPC messages, capability identifiers and compatibility rules are defined in
[`crates/plugin-protocol`](../crates/plugin-protocol); the async client and provider trait are in
[`crates/plugin-sdk`](../crates/plugin-sdk). Trusted Steam policy and packaging details are
maintained alongside the integration implementation.
