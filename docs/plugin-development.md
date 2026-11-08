# Plugin development

GameNode Desktop starts bundled discovery plugins on demand. The Tauri process owns the Engine,
the profile and persistence; a plugin only reports observations through the versioned
`gamenode.plugin.v1` API. A plugin never calls `gamenode-core`, writes the database, or receives
GameNode login credentials.

## Discovery contract

The host launches one registered executable for a provider and gives it a private socket path,
operation id and temporary credential through its environment. The plugin opens a session with its
stable plugin id, API version and one requested capability, scans outside the host's UI thread, and
submits one terminal discovery result or failure. A completed result includes the registered
`provider_id` and records containing:

- `id`: stable id within that provider (for Linux package discovery this is the package id).
- `name`: optional display title.
- `install_dir`: installation directory required by the current Engine record model.
- `fields`: additional string values for provider use.

The host checks plugin identity, provider, operation, session credential, field sizes and record
count before routing records through normal Engine matching and persistence. Plugins should skip
individual malformed entries and return other valid results. A successful empty result means the
provider was scanned and no candidates were found. A failed scan preserves that provider's previous
successful data.

## Current capabilities

| Capability | Granted to | Scope |
| --- | --- | --- |
| `library.discovery` | Official Steam scanner and Linux package manager | Submit results for the provider and active scan operation authorized by the host. |

There is no general Core, database, REST, credential, filesystem, or process API permission. The
Linux package manager plugin uses its distro backend under the logged-in user's normal process
identity; the Plugin API capability does not grant extra operating-system privilege.

## Adding an official provider plugin

1. Create a workspace executable crate. Depend on `gamenode-plugin-protocol` and reusable scanner
   crates; do not depend on `gamenode-core` or `gamenode-platform`.
2. Add the provider manifest under `providers/`. Use a native scanner placeholder only when the
   provider is always routed to an external plugin. Keep the manifest limited to provider metadata
   and supported actions.
3. Register the provider id, stable plugin id, exact capability and packaged executable in the
   host descriptor table in `crates/app/src/plugin_host.rs`. The API handshake and the launched
   operation must agree on all four values.
4. Implement the handshake and `SubmitDiscovery` flow using `crates/plugin-protocol`. Include the
   provider id in every completed result. Never log session credentials or private scan contents.
5. Add the binary to `scripts/package-linux.sh` and the Linux install/uninstall packaging paths.
   Linux plugins use the authenticated per-user Unix socket. Windows plugin transport is not
   implemented; Steam remains in-process there.
6. Update this guide, `docs/providers.md`, development guidance and the `host-tauri` OpenSpec tasks.

Official plugin executables are trusted native code, not sandboxed extensions. A same-user process
can access files and start programs allowed to that user. Plugin installation, community trust,
permission prompts, sandboxing and Windows named pipes require separate product work.

## Linux package manager scanner notes

The plugin examines installed desktop entries categorized as games and accepts only entries owned by
an installed package with a resolvable executable. On Arch-based distributions it queries pacman
directly; other distributions use PackageKit. This may omit packages when PackageKit lacks file
ownership search or desktop metadata is incomplete. An unavailable selected backend fails only that
provider scan.
