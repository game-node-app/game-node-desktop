# Developing a GameNode provider plugin

GameNode's provider API lets a bundled provider process implement store-specific behavior while
the desktop application remains responsible for the library, profile, persistence and GameNode
account. The process communicates with the running desktop through the versioned public crates
`gamenode-plugin-protocol` and `gamenode-plugin-sdk`. Provider plugins must not depend on
`gamenode-core`, `gamenode-app`, `gamenode-api` or `gamenode-platform`; they must not read or write
the GameNode database or GameNode credentials.

## Official plugins and trusted descriptors

Official plugins are compiled-in and trusted. Each one is described by a `plugins/<name>.toml`
descriptor that `crates/app/build.rs` embeds together with the provider manifest it names and the
login policy it references. The host parses and validates the descriptors once at startup; nothing
is read from disk at runtime. Adding a store therefore needs a plugin crate, a
`providers/<provider id>.toml` manifest and a `plugins/<name>.toml` descriptor, not a new Rust
branch in the host.

The bundled plugins are:

- **Steam** (Windows and Linux): installed discovery, account sign-in/check/sign-out, owned and
  Family catalog sync, playtime observations, Play/Install actions and achievement reads.
- **Linux package manager** (Linux): installed package candidates through PackageKit or pacman.
  It has context and discovery grants only.
- **[Epic Games Store](epic-games-store.md)** (Windows): installed discovery, launcher actions,
  owned catalog, playtime totals and an opt-in account.

The manual DRM-free provider is built into Core and is not a plugin.

A descriptor has these fields:

| Field | Meaning |
| --- | --- |
| `provider_id` | Stable provider id. It must be a plain lowercase id (`a-z`, `0-9`, `-`, `_`). |
| `plugin_id` | The plugin's public id, matched against the executable's handshake. |
| `name` | Display name used by inventory, Stores and provider-neutral copy. |
| `platforms` | Platforms where the plugin is available. They must be a subset of the manifest's platforms. |
| `capabilities` | The host-service grants approved for this provider. |
| `executable` | File name of the executable placed next to the desktop host. |
| `provider_manifest` | Provider id of the embedded `providers/<id>.toml` manifest. It must equal `provider_id`. |
| `login_policy` | Optional repo-relative path to an embedded login policy. Present exactly when `auth.webview` is granted. |
| `credential_alias` | Optional existing keyring account and private file for a provider that must keep its historical physical credential. |
| `native_access` | Declarations of OS access the plugin itself needs. They document intent; they are not sandbox rules. |
| `legacy_login_profiles` | Profile directory names from older builds that startup cleanup removes. |

Descriptors are validated before use. A descriptor is skipped, with a diagnostic, when its manifest
is missing or its id differs, it lists a platform its manifest does not, its login policy presence
does not match `auth.webview`, it grants `auth.webview.cookies.read` without a policy that declares
cookie names, it names an unknown or duplicate capability, its provider id duplicates another
descriptor, or its credential alias is not a plain provider-specific name (including the GameNode
session names). Every other descriptor still loads. Inventory, provider backends and test fixtures
all derive from the same validated data.

## Session and operations

The host starts a registered plugin process and provides its local endpoint, a temporary session
credential and an operation identifier through the environment. The plugin opens a session with
`Session::open(Launch::from_env()?, plugin_id, provider_id, version, capabilities)` and implements
the async `ProviderPlugin` trait. `run(session, Arc::new(plugin))` dispatches host commands. Use the
`HostClient` scoped to each command for approved services, and return an `OperationResult` through
the SDK. Do not retain a command's client to authorize later work: operation, account and profile
generations fence stale requests.

The current public API is major 1, minor 5. Minor 5 adds optional last-played counter values,
catalog game fields, the `invalid credential` catalog failure and cookie-free login policies.
Minor 4 added achievement reads; minor 3 added typed settings; minor 2 added the full operation
set. A plugin negotiates its newest supported minor within major 1, and the host grants the
intersection of the requested capabilities with its trusted descriptor. Unknown or duplicate
permissions are rejected in `gamenode-plugin-protocol`.

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
| `auth.webview.cookies.read` | Read only the approved cookie names for the active login attempt. Required only when the trusted policy declares cookies. |
| `auth.webview.page.read` | Read only the bounded page probe at the exact origin in the trusted login policy. Plugins cannot submit arbitrary scripts or origins. |
| `secrets.read` | Read this provider's logical secret slot. |
| `secrets.write` | Write this provider's logical secret slot. |
| `secrets.delete` | Delete this provider's logical secret slot. |
| `playtime.counter` | Return provider-local total-playtime and last-played observations. Core retains tracking, attribution, deduplication and submission policy. |
| `provider.actions.declare` | Declare approved provider actions that the host validates against trusted URI schemes and templates. It does not authorize arbitrary commands, shells or filesystem changes. |
| `achievements.read` | Return bounded per-game achievement definitions and the connected account's unlock state for this provider. The host rejects unrequested game ids, oversized batches and strings, and results fenced by an account change. Available from API minor 4. |

A provider should request only the capabilities it actually needs. Capability names are not
free-form: a descriptor containing an unknown name is invalid and is not loaded.

## Provider secrets

Every provider sees one logical secret slot named `session`; the host rejects any other key. A
plugin cannot choose a physical keyring account, file name or another provider's slot.

The physical slot is derived from the provider id, unless the trusted descriptor declares an alias:

- **Default keyring account:** `provider.<provider_id>`.
- **Default Linux private file:** `private/provider-<provider_id>.secret` below the application data
  directory.
- **Steam's legacy alias:** `credential_alias = { keyring_account = "steam", private_file = "steam-token" }`.
  Existing Steam sessions keep the same physical credential, with no migration, renaming or
  re-sign-in.

The GameNode account session remains in its own `session` keyring account and
`private/session-token` file. Descriptor aliases may not reuse those names. On Windows the host
uses Credential Manager and has no plaintext fallback. On Linux it uses Secret Service and falls
back to a user-only private file only when no Secret Service exists; a locked or failing service is
reported as an error. Secret read, write and delete are independent grants, and a reported storage
method is not proof that a write was durable.

## Login policies and cookie-free extraction

`auth.webview` opens an isolated window with an ephemeral profile scoped to the provider. URLs,
allowed hosts, the page probe and any cookies come from the trusted login policy. Runtime requests
supply only window handles and operations. External HTTPS links outside the approved hosts open in
the system browser. The probe runs only at the exact `probe_origin`, and a navigation change
invalidates an in-flight result.

A policy may read cookies or only approve a page probe:

- `cookie_urls` is an optional list of URLs to read cookies from. When non-empty,
  `cookie_names` must be non-empty; when empty, `cookie_names` must be empty. Several URLs let a
  policy capture a session whose cookies Steam scopes to different hosts.
- `auth.webview.cookies.read` is valid only with a policy that declares cookie names. A
  cookie-free policy does not grant it.
- A login `Read` on a cookie-free policy skips cookie collection entirely and returns only
  `page_result_json`. This supports authorization-code flows where the plugin receives a one-time
  code instead of a cookie.
- `page_result_json` is bounded JSON and is secret-bearing. The host never logs it and never
  includes it, or any cookie value, in tracing output or GUI snapshots.

Each attempt uses `private/login/<provider>` and the host removes that directory when the attempt
ends, whether it succeeded, was cancelled or timed out. At startup the host also removes the
`legacy_login_profiles` named by trusted descriptors. A newer attempt's profile is never deleted by
a late cleanup from an older attempt.

## Host capabilities and native access are different

Host capabilities authorize calls to explicit GameNode services. They are scoped to the plugin,
provider, process session and operation, and the host checks the grant on each call. For example,
`secrets.read` authorizes a request for the plugin's logical secret; it does not authorize reading
arbitrary files or the operating system's credential database directly.

Native-access declarations describe what the trusted bundled executable needs to access for its
own implementation. A store plugin may need HTTPS access to the store's API, reads of store
library and configuration files (including supported Linux native/Flatpak locations), the store
install root from the Windows registry, or process and D-Bus access. These declarations document
expected access; they are not operating-system sandbox rules. The plugin is an ordinary process
running with the user's OS permissions, so filesystem and network access by that process are not
confined by the host API. Community code is not enabled by these bundled-plugin permissions or
declarations.

## Implementation rules for store plugins

- Keep store parsing, account/session validation, HTTP calls, login interpretation, discovery and
  the store's own playtime counter inside that store's plugin. Use the public host services for
  login windows, provider secrets, account state, discovery/catalog results and action
  declarations.
- Keep GameNode authentication, matching, library reconciliation, favorites, artwork, profile
  persistence, playtime attribution and submission policy in the desktop/Core.
- Store login URLs, allowed hosts, cookies and page probes come from trusted packaged policy data.
  Runtime requests must not replace that policy. Do not log cookies, tokens, authorization codes,
  credentials, raw secret-bearing messages or complete account URLs.
- Distinguish a successful empty result from a failed or incomplete operation. A failure must not
  be reported as an empty catalog or discovery result; the host preserves prior successful facts
  according to the operation's contract.
- Keep blocking file, registry and credential access off the UI thread. Network work and user
  login waits must remain cancellable and must not block the desktop event loop.

The concrete DTOs, RPC messages, capability identifiers and compatibility rules are defined in
[`crates/plugin-protocol`](../crates/plugin-protocol); the async client and provider trait are in
[`crates/plugin-sdk`](../crates/plugin-sdk). Trusted descriptors live in [`plugins`](../plugins),
and provider manifests live in [`providers`](../providers).
