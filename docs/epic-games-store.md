# Epic Games Store

GameNode reads Epic Games Store games on Windows through the official Epic plugin
(`gamenode-plugin-epic`). Installed discovery, launcher actions, the account, the owned catalog and
Epic playtime totals all live in that plugin. No host crate reads Epic launcher files, interprets
Epic HTTP responses or branches on the `epic` provider id; the host only loads the provider
manifest and the trusted plugin descriptor.

| Concern | Path |
| --- | --- |
| Provider manifest | `providers/epic.toml` |
| Trusted plugin descriptor | `plugins/epic.toml` |
| Trusted login policy | `crates/plugin-epic/assets/login.toml` |
| Installed discovery | `crates/plugin-epic/src/discovery/` |
| OAuth, library, catalog and playtime HTTP client | `crates/plugin-epic/src/http.rs` |
| Provider operations and plugin lifecycle | `crates/plugin-epic/src/runtime.rs` |

The plugin compiles and runs its fixture tests on Linux, but its descriptor lists `windows` only.
On Linux the provider, its account card and its inventory entry are absent, and no Epic executable
is packaged.

## Installed discovery

The plugin asks the host for the provider context, reads the roots declared by
`providers/epic.toml`, and expands environment placeholders itself:

1. `HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher`, value `AppDataPath`. This is where the
   launcher records its data directory; it uses the 32-bit machine key even on 64-bit Windows.
2. `%ProgramData%\Epic\EpicGamesLauncher\Data`, for portable or older installs without that value.

The first existing directory is used. It reads every `*.item` JSON manifest in `<data>\Manifests`.
A game is reported when all of these hold:

- `AppName` and `InstallLocation` are non-empty.
- `bIsIncompleteInstall` is not `true`.
- `MainGameAppName` is empty or equals `AppName`.
- `AppCategories` contains `games`.
- Neither `CatalogNamespace` nor `Namespace` is the engine namespace `ue`.

Records use `id = AppName`, `name = DisplayName`, `install_dir = InstallLocation`, and the fields
`namespace` (`CatalogNamespace`) and `catalog_item_id` (`CatalogItemId`) when present. Duplicate app
names keep the first record. Malformed manifests and DLC, engine or incomplete items are skipped
individually and counted in a secret-free log; they do not stop the rest of the scan.

A missing launcher data directory is a successful empty result, which clears the previous installed
facts. A `Manifests` directory that exists but cannot be read is a failure, so the previous
installed facts and matches remain visible. All filesystem work runs on the plugin's blocking
workers, never on the UI thread.

## Launcher actions

The manifest declares two templates, filled from the record id by the host:

```text
com.epicgames.launcher://apps/{id}?action=launch&silent=true
com.epicgames.launcher://apps/{id}?action=install
```

Play is offered for installed entries and Install for owned entries that are not installed. The
host percent-encodes the id, validates the filled URI and hands it to the operating system as one
argument. Nothing else is executed. If no handler is registered for the `com.epicgames.launcher`
scheme, the action reports that the Epic Games Launcher is required and the library is unchanged.

## Sign-in

Epic sign-in is opt-in and uses the host's isolated sign-in window. The trusted policy in
`crates/plugin-epic/assets/login.toml` is cookie-free:

```toml
start_url = "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D<launcher client>&responseType=code"
probe_origin = "https://www.epicgames.com"
allowed_hosts = ["epicgames.com", "epicgames.dev", "unrealengine.com", "hcaptcha.com"]
cookie_names = []
```

- The policy has no `cookie_urls`. The host therefore skips cookie collection entirely; the login
  window never returns cookies to the plugin.
- Navigation stays in the window only on the allowed Epic hosts. A link to any other host, which is
  how social sign-in providers are reached, opens in the system browser and cannot complete the
  Epic sign-in.
- The window runs one bounded page probe at exactly `https://www.epicgames.com`. The probe accepts a
  result only on `/id/api/redirect` and only when the page body parses as JSON with a non-empty
  `authorizationCode`. It returns `{ "authorization_code": "…" }`; every other page, parse failure
  or missing code returns `null`. It does not read cookies, local storage or the URL query.
- The plugin exchanges the one-time code at Epic's token endpoint and drops it immediately. The
  code, tokens and account identifiers are never logged, shown in a snapshot or written to the
  database.

Supported methods are Epic's own email/password form, including Epic's two-factor and captcha
steps. `hcaptcha.com` is allowed so the captcha widget can load. Sign-in with a linked social
provider is not supported inside the window: Google, Apple, console and similar providers leave
Epic's hosts, open externally and leave the window without an Epic session.

Closing the window, cancelling or timing out saves no credential, and the previous account state is
unchanged. A late result that arrives after cancel or sign-out is reported as superseded and never
writes a credential.

## Credential storage

Only the rotating refresh token is persisted:

- It is written to the plugin's logical `session` secret. Epic's descriptor declares no
  `credential_alias`, so the host uses the provider's default slot: the `provider.epic` account in
  Windows Credential Manager, or the `provider-epic.secret` file in the private directory where a
  Linux fallback would apply.
- The access token and the account id stay in memory only.
- Every successful refresh returns a new refresh token. The plugin writes the rotated token before
  publishing the account as connected, so a restart restores the account without another sign-in.
- `invalid_grant` and an HTTP 400/401 from the token endpoint delete the dead secret and mark the
  account expired; cached library facts remain visible.
- A network failure or a 5xx keeps the secret and marks the account unreachable.
- Sign-out deletes the local secret first, then best-effort revokes the Epic session with a short
  timeout.

Secrets, authorization codes, tokens and account ids are never written to SQLite, settings,
snapshots, logs or error text. Logs name only a static endpoint label, the HTTP status and the body
size.

## Requests

Epic publishes no launcher API. These endpoints mirror what the official launcher and open-source
launchers use, so they are **unofficial** and may change. They are isolated in
`crates/plugin-epic/src/http.rs`; every response is parsed narrowly and every failure becomes a
category rather than a guess.

The client uses rustls, a 20-second request timeout, no redirect following (a redirect could carry
an authorization header to another host), and capped response bodies. The OAuth client id and
secret are the public Epic Games Launcher constants already published by open-source launchers;
they identify the launcher application, are not user secrets, and are shared by every install.

| Request | Purpose |
| --- | --- |
| `POST https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token` | Exchange the authorization code or the stored refresh token. Uses Basic auth with the launcher client and form fields `grant_type`, `code`/`refresh_token`, `token_type=eg1`. |
| `DELETE https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/sessions/kill/{access_token}` | Best-effort session revoke on sign-out. |
| `GET https://library-service.live.use1a.on.epicgames.com/library/api/public/items?includeMetadata=true&cursor=…` | Page through the owned library. `includeMetadata=true` makes each record carry its namespace and catalog item id. |
| `GET https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace/{namespace}/bulk/items?id=…&includeMainGameDetails=true&country=US&locale=en-US` | Resolve titles and DLC/engine status for one namespace. |
| `GET https://library-service.live.use1a.on.epicgames.com/library/api/public/playtime/account/{accountId}/all` | Epic's cumulative playtime ledger, in seconds, keyed by artifact id. |

The owned library drops engine (`ue`) and non-`appName` records, and excludes items whose catalog
entry has a `mainGameItem` or lacks the `games` category. Any paged request or title resolution that
still fails after one retry fails the whole owned category, so the previous owned facts remain
instead of being replaced by a partial library. There is no family category.

## Matching by namespace

`providers/epic.toml` declares:

```toml
match = { external_category = 1001, uid_field = "namespace" }
```

Category `1001` is `EpicGamesNamespace` in `game-node-server`
(`src/game/game-repository/game-repository.constants.ts`). Its uids are 32-hex catalog namespaces,
for example GTA V's `0584d2013f0149a791e7b9bad0eec102`. IGDB's `EpicGamesStore` category `26` is a
different category with store slugs, so it is not used here.

- Installed and owned records both carry `namespace` and `catalog_item_id`.
- The lookup sends the entry's `namespace`; an entry without a namespace is not looked up and stays
  unmatched.
- One answer applies to every entry of the provider with that namespace, so several apps of one
  product can share a match.
- A user's manual match always wins.
- The stored entry key stays `epic:<AppName>`, because launch, install and playtime key on the
  application name and several applications can share a namespace.

## Playtime

With a connected account, the plugin reads Epic's cumulative totals and reports them through
`playtime.counter`. Epic's ledger is keyed by artifact id, while GameNode entry keys, playtime
display and submissions are keyed by `epic:<AppName>`. The plugin therefore maps every artifact id
to the application name recorded in the owned library and reports `id = AppName`, never the raw
artifact id:

- The mapping is built from the owned library records on each successful catalog sync and kept with
  the account state. It is cleared when another account connects, on sign-out, and when the
  credential expires or is removed; it is kept across a failed sync, so a later successful check can
  resolve totals without another full sync.
- Only artifact ids that map to a library application name are reported. An artifact that no longer
  appears in the owned library is skipped, and a raw artifact id never becomes an entry id.
- If several artifacts belong to one application name, their totals are added into that one
  observation.
- `seconds = totalTime` and an absent `last_played`. An absent date is stored as unknown rather than
  1970, so an Epic game that has no played date does not show a fake date.
- The manifest source is `epicgames`, the value `game-node-server` already uses for its own Epic
  connection. Submissions therefore use the server's existing Epic source and platform 6, and a
  user with both connections gets idempotent writes rather than double counting.
- When several applications of one provider match the same GameNode game, the host sums their
  totals into one submission for that game and keeps the latest known played time, matching how the
  server aggregates Epic apps per namespace.
- Without a connected account or credential the plugin returns no observations. That is not a zero
  total: a previously accepted total is not overwritten.

The design's Decision 10 wording, "id = artifactId", conflicts with the `desktop-epic-games` spec,
which requires totals per application name. The implementation follows the spec: the entry key,
display and submission matching all use `appName`, so reporting the artifact id would never match
an entry. The server solves the same mismatch by mapping `artifactId` to `appName`.

## Known limitations

- **Windows only.** Epic on Linux (Heroic, Legendary) is out of scope. The provider and the
  executable are excluded from Linux builds, and no Epic control appears there.
- **The Epic Games Launcher is required** for Play and Install. GameNode does not launch games
  without the launcher.
- **Social sign-in is unsupported** inside the isolated window. Email/password with 2FA is the
  supported path; social providers open externally.
- **The endpoints are unofficial** and can change or be rate limited. A changed response becomes a
  categorized failure; previously imported games stay cached and installed discovery keeps working.
- **No achievements, cloud saves or DLC management.** Epic DLC is excluded from discovery and the
  owned catalog, and Epic does not report a family category.

## Automated coverage

- `crates/plugin-epic/src/discovery/tests.rs` covers two games, DLC, an incomplete install, an
  engine item, a malformed manifest, a missing launcher and an unreadable directory.
- `crates/plugin-epic/src/http/tests.rs` covers the code exchange, refresh and rotation,
  `invalid_grant`, 401, 429, 5xx, malformed bodies, multi-page cursors and secret-free errors.
- `crates/plugin-epic/src/runtime/tests.rs` covers the counter mapping from Epic artifact ids to
  library application names, the skip of an unmapped artifact, retention across a failed sync and
  clearing when another account connects.
- `crates/core/src/provider/registry/tests.rs` pins the Epic manifest (category `1001`, source
  `epicgames`, the namespace uid field, the HKLM root and the `%ProgramData%` fallback), the
  Windows-only trusted descriptor and the cookie-free login policy.

Real launcher URIs, the Epic sign-in page inside WebView2, captcha and 2FA, a large owned library
and token rotation still need a Windows machine with a real account; they are part of the change's
acceptance pass.
