# Desktop authentication wire contract

GameNode Desktop reuses the server's existing mobile browser sign-in flow. This
document records the observed contract for the Rust client; it does not define
new server endpoints or the separate MCP/client-connection authorization flow.

## Deployment prerequisite

The server's Better Auth configuration builds `trustedOrigins` from the website
origin and the comma-separated `TRUSTED_ORIGINS` setting. Each deployment used
by Desktop must include the exact native origin `app.gamenode://` in that
setting. The server integration suite has a dedicated “allowed client origins
and native app scheme” group for this origin. A production deployment that
doesn't trust it may reject native sign-in even though browser sign-in works.

## Browser sign-in

1. Desktop starts a short-lived attempt and builds exact success and failure
   callback targets, carrying its single-use `desktop_attempt` correlation
   value:

   - `app.gamenode://home?desktop_attempt=<nonce>`
   - `app.gamenode://home/sign-in-failed?desktop_attempt=<nonce>`

2. It sends `POST /v1/auth/sign-in/social` with
   `capacitor-origin: app.gamenode://` and a JSON body containing `provider`,
   `callbackURL`, and `errorCallbackURL`. The existing provider IDs are
   `google`, `discord`, and `twitter`.

3. The response supplies an authorization URL. If its `Set-Cookie` headers
   include `better-auth.oauth_state` (including the `__Secure-` prefixed form),
   Desktop carries that cookie value to the server's existing
   `/v1/auth/capacitor-authorization-proxy` as `oauthState`. It also supplies
   the URL as `authorizationURL`. The system browser opens this proxy URL so
   the server can establish OAuth state in the browser before redirecting to
   the provider.

4. The server redirects to the registered callback. A successful callback
   carries the `cookie` parameter containing the encoded session `Set-Cookie`
   header. Desktop accepts the `better-auth.session_token` cookie or its
   `__Secure-` form, extracts the signed value, and validates it using
   `GET /v1/auth/get-session` with that value as a bearer token. A failure
   callback uses `/sign-in-failed` and does not establish a session.

5. Only after session validation does Desktop persist the credential in the OS
   credential service (or the documented Linux-only fallback) and show the
   returned identity. Sign-out attempts `POST /v1/auth/sign-out` with the same
   bearer credential, then removes local authentication regardless of server
   availability.

The callback URL and credential are untrusted input until the active attempt's
target, nonce, expiry, and single-use state are checked. Never log a complete
callback URL, cookie, or credential. The local protocol fixtures use fake,
redacted values and cover the encoded cookie and bearer handling.

## Source and fixture references

Sibling server sources, relative to the desktop repository root:

- `../game-node-server/src/auth/auth.service.ts` — `/v1/auth` configuration,
  social providers, `bearer()` and `capacitor()` plugins, and trusted origins.
- `../game-node-server/src/auth/auth.integration.spec.ts` — Better Auth
  integration coverage, including the native app scheme/origin.
- `../game-node-server/src/auth/mount-auth.ts` and
  `../game-node-server/src/main.ts` — mounting the existing auth handler.

Desktop implementation and fixture sources:

- `crates/api/src/lib.rs` and `crates/api/src/tests.rs` — request headers,
  callback fields, proxy handoff, OAuth-state transfer, bearer session lookup,
  and sign-out.
- `crates/core/src/callback.rs` — callback targets, accepted cookie names,
  parsing, and redacted rejection cases.

The implemented flow uses only the existing sign-in, proxy, session, and
sign-out endpoints. It does not add server endpoints or use MCP/client
connection registration.
