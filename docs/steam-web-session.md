# Steam web session and owned library

GameNode imports a Steam account's owned and Steam Family games without a Steam Web API key. The
only stored secret is the Steam store session cookie. A short-lived web API token is derived from
the authenticated store page on every check and never persisted.

The client lives in `crates/api/src/steam.rs` and implements the `SteamWebApi` port from
`crates/core/src/steam.rs`. It uses the same HTTP stack as the GameNode client: rustls, the system
proxy, a realistic desktop Chrome User-Agent, a 20-second request timeout, redirects limited to a
Steam host, and no cookie store (the cookie header is always explicit).

Two details keep it equivalent to Playnite's WebView, which is the reference for this flow:

- **Response decompression.** Steam's store front answers some requests with a gzip body even when
  the client did not advertise `Accept-Encoding` (the recommendation queue does this for its
  redirect and page responses alike). A client that does not decode `Content-Encoding` sees binary
  garbage, fails the UTF-8 check and reports `changed` on every check, no matter how fresh the
  cookie is. The client enables gzip, deflate, brotli and zstd like a browser does.
- **Browser request headers and cookie bootstrap.** Store page requests send a Chrome-style
  `Accept` and `Accept-Language`; the token fallback also sends the store `Referer`. More
  importantly, Steam replies to a request that lacks its frontend cookies with a 302 back to the
  same URL and `Set-Cookie`; the client keeps a request-local cookie jar across those bounces (see
  "Validate the session" below). Playnite's WebView has that jar already.

When a store page does not match the parsed contract the client logs a secret-free shape:
status, `Content-Type`, `Content-Encoding`, body size and whether the `application_config`,
`data-userinfo` and `data-store_user_config` markers were present. Bodies, cookies and tokens are
never logged.

## Stored credential

The credential is the minimal store cookie header:

```text
steamLoginSecure=<steamid>%7C%7C<session secret>; sessionid=<session id>
```

- `steamLoginSecure` is percent-encoded; `%7C%7C` is the literal `||` between the SteamID and the
  session secret. The SteamID can be recovered from it when the store page cannot be parsed.
- `sessionid` is the accompanying store session cookie.
- Other cookies the store returned (for example `steamCountry`) are preserved if present.
- Steam rotates either cookie through `Set-Cookie`. The client replaces the rotated values,
  keeps the rest of the header, and returns the updated header in `SteamSession::cookie` so the app
  can persist it before the catalog sync finishes.
- Neither the cookie nor the token is written to SQLite, settings, snapshots, logs or error text.

## Requests

### 1. Validate the session

```http
GET https://store.steampowered.com/explore/
Cookie: <stored header>
```

- Uses the same authenticated store page as Playnite's web login flow. Follows at most 10 redirects;
  only Steam hosts (and loopback hosts for tests) are followed, with repeated destinations stopped.
- Steam bootstraps a session-less client with a redirect to the **same URL** plus `Set-Cookie` for
  `steamCountry`, `browserid` and `Steam_Language`. That self-redirect is not a loop: the client
  merges every `Set-Cookie` from the response into a request-local cookie header and resends the
  same URL, up to three times. Those frontend cookies are never persisted; only
  `steamLoginSecure`/`sessionid` rotate into the stored credential.

  This is why a client without a cookie jar sees a 302 on every store request (and Playnite's
  WebView does not): the bootstrap cookies are the difference between the 302 and the real page.
- A final URL under `/login` means the session expired. So does a redirect whose `Location` is
  under `/login`, even one the client does not follow (another scheme or host).
- Reads at most 4 MiB of decoded UTF-8.
- Finds the `<div id="application_config" ...>` start tag and reads:
  - `data-userinfo` JSON: `logged_in`, `steamid`, `account_name`.
  - `data-store_user_config` JSON: `webapi_token`.
- HTML entities (`&quot;`, `&amp;`, decimal and hexadecimal entities) are decoded before JSON
  parsing.
- `logged_in == false` means `expired`. A missing/unparseable attribute or an empty token makes the
  client try the store home page (`GET https://store.steampowered.com/`, same contract) before the
  token fallback below. The recommendation queue is Playnite's page; the home page keeps working if
  that page changes shape.
- `Set-Cookie` values for `steamLoginSecure` and `sessionid` become the rotated cookie header.

### 2. Token fallback

```http
GET https://store.steampowered.com/pointssummary/ajaxgetasyncconfig
Cookie: <stored or rotated header>
```

Expected body:

```json
{ "success": 1, "data": { "webapi_token": "..." } }
```

- Used only when the store page has no parseable `application_config`/token.
- `success != 1`, a missing `data`, or an empty token means `expired`.
- The fallback has no identity fields, so the SteamID is decoded from `steamLoginSecure`; the
  account name is left empty and the app keeps the last known name from `store_accounts`.

### 3. Owned games

```http
GET https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/
    ?access_token=<webapi_token>
    &steamid=<steamid>
    &format=json
    &language=english
    &include_appinfo=true
    &include_played_free_games=true
    &include_free_sub=false
    &skip_unvetted_apps=false
```

- Keeps numeric app ids with a non-empty name, trims whitespace, strips `™`, `®` and `©`, and
  deduplicates by app id.
- A 200 answer with a missing `response`/`games` array is `privacy`: Steam did not disclose the
  library. `game_count == 0` with an empty `games` array is a successful empty catalog.
- 401/403 is `expired`.

### 4. Steam Family group

```http
GET https://api.steampowered.com/IFamilyGroupsService/GetFamilyGroupForUser/v1/
    ?access_token=<webapi_token>
    &steamid=<steamid>
```

- `is_not_member_of_any_group == true` is a successful empty family catalog.
- Otherwise `family_groupid` is used for the shared-apps request. A missing group id is also
  treated as "no family".

### 5. Steam Family shared apps

```http
GET https://api.steampowered.com/IFamilyGroupsService/GetSharedLibraryApps/v1/
    ?access_token=<webapi_token>
    &steamid=<steamid>
    &family_groupid=<group id>
    &language=english
    &include_own=false
    &include_excluded=false
    &include_free=false
    &include_non_games=false
    &max_apps=10000
```

- Keeps entries with a numeric app id, a non-empty cleaned name and `exclude_reason == 0`.
- Excluded apps are counted in a debug log but neither their names nor the response are logged.
- Duplicates are removed. The app merge treats owned as winning over family-shared.
- A family failure is isolated: owned catalog updates still apply, and the app records the family
  error as `familyUnavailable`.

## Token lifetime

The `webapi_token` is short-lived (roughly one day, and Steam may rotate it sooner). GameNode never
stores it: every startup check, Stores check and catalog sync validates the cookie and reads a fresh
token first. An API 401/403 makes the account `expired`, and the app removes the dead credential
while keeping the cached installed/owned/family facts visible.

## Retries and error categories

All five requests share the retry policy:

- 429 and 5xx are retried at most three times (four attempts total) with a 100 ms, 200 ms, 400 ms
  backoff.
- A numeric `Retry-After` is honored up to five seconds; a long or date-valued header falls back to
  the backoff.
- After the retries: 429 is `rateLimited`, 5xx is `server`.

Failure categories (`SteamError`):

| Category | Meaning |
| --- | --- |
| `unreachable` | Connection failed or timed out |
| `rateLimited` | 429 after the bounded retries |
| `server` | 5xx or another unexpected HTTP status after retries, including a redirect that left the Steam hosts or exceeded the limit |
| `expired` | Login redirect, `logged_in == false`, empty fallback data, or 401/403 |
| `privacy` | 200 owned response without a disclosed library |
| `changed` | The store page or JSON no longer matches the parsed contract |
| `invalidCookies` | The stored cookie header is not a valid HTTP header value |

`SteamError` maps to the stable `CatalogError` categories; family failures use
`FamilyUnavailable`. Errors and logs contain the endpoint path (never the query string), the HTTP
status and the category. They never contain the cookie, the token, a full URL, a header or a page
body. The one location detail is a redirect the client did not follow: its host and path are
logged as `Steam redirect was not followed`, without the query string.

## Breakage behavior

Steam's store HTML and the family endpoints are unofficial. The client parses a narrow contract and
degrades instead of guessing:

- A store page layout change becomes `changed`, after the JSON token fallback has been tried. The
  app shows "Steam changed its response"; previously imported owned and family games stay cached and
  installed games are unaffected.
- A token or cookie rejection becomes `expired`; the app keeps the cached library and asks for a
  new sign-in.
- A network outage becomes `unreachable`; the credential is kept and the cached library remains
  visible.
- Rate limits and server errors are retried briefly and then reported without a second sync
  starting.
- Steam Family can fail independently; the owned catalog and installed scan still complete.

## Manual acceptance still required

The automated coverage uses `httpmock` fixtures: logged-in and logged-out store pages, gzip-decoded
store pages and token fallbacks, browser request headers, the home-page fallback, entity decoding,
rotated cookies, fallback token, missing attributes, the 4 MiB cap, owned/empty/private libraries,
401/403, family/no-family/excluded apps, rate-limit recovery, bounded 5xx retries, and the
secret-free error/log assertion.

The following cannot be verified in this Linux container and still need a real account and OS:

- Real Steam Guard sign-in and the isolated WebKitGTK/WebView2 login window.
- A real 300+ game library, its import time and the installed-first cover queue.
- A real Steam Family group, invited/excluded apps and leaving a group.
- Cookie and token rotation across a real session, plus Steam's actual rate limits.
- Linux: Secret Service storage versus the 0600 private-file fallback and its warning.
- Windows: Credential Manager storage, WebView2 sign-in and Steam Guard.

Record those results in the change notes when the acceptance pass runs.

## Verification record

2026-11-07, real account on Windows, after the cookie-bootstrap fix:

- `session()` returned the SteamID, the account name, a web API token and the minimal persisted
  cookie (no bootstrap cookies) while Steam answered the store request with a self-redirect first.
- The owned catalog returned 100 games and the Steam Family catalog 1330 games with a group id.
- The app card showed `Connected` with 100 owned, 1282 shared and 6 installed games, and the
  credential stored in Windows Credential Manager.

Still manual: Steam Guard sign-in inside the real WebView2/WebKitGTK window, credential storage on
each OS, and observing Steam rotate the session cookie during a long-lived session.
