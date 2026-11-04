# Desktop IPC and callback lifecycle

The app acquires per-user instance ownership before opening SQLite. A later
launch sends a bounded `activate` or `auth-callback` message to that owner and
exits. The owner raises its window and validates callback messages as data; IPC
delivery by itself never authenticates a user.

## Per-user endpoints

| Platform | Endpoint | Access rules |
| --- | --- | --- |
| Windows | `\\.\pipe\gamenode-desktop-<user-SID>` | The pipe DACL grants access only to the current user and SYSTEM. Remote clients are rejected. The SID in the pipe name separates Windows users. |
| Linux | `$XDG_RUNTIME_DIR/gamenode-desktop/instance.sock`, with `instance.lock` beside it | The app-owned directory is mode `0700`; the lock and socket are mode `0600`. The directory must belong to the current UID. If `XDG_RUNTIME_DIR` is missing or not absolute, the app uses `gamenode-desktop-<uid>` under the OS temporary directory. |

The Linux lock is released by the kernel when its process exits. A new owner
removes a stale socket only after acquiring that lock. The Unix socket path is
bounded for `sockaddr_un`; unsafe or overlong runtime locations fail startup.
Messages have a bounded frame, reads have a two-second timeout, at most 16
connections are served concurrently, and a second process waits up to five
seconds for an owner that is still starting. A timeout or rejected message does
not change authentication state.

## URI delivery and cold starts

The registered URI handler passes the complete `app.gamenode://` URI as one
argument. If another instance owns the app, the launching process forwards the
URI unchanged and exits. If no owner exists, that process becomes the owner,
initializes local state, starts the engine, then dispatches its original URI to
the same callback validator used for forwarded callbacks.

Pending browser attempts are intentionally in memory and are not restored after
a restart. A callback that cold-starts the app therefore has no matching active
attempt: its scheme, callback target, nonce, expiry, and single-use state are
checked, then authentication is rejected without storing its credential. The
callback cannot replace an existing identity either.

Completed-session restoration is separate. On startup the engine loads the
saved credential from the OS credential store (or the documented Linux
fallback) and asks GameNode to verify it. A valid credential restores the
identity; a rejected credential is deleted; a temporary network failure keeps
it and offers retry. The app does not persist a partial OAuth attempt or treat
an incoming URI as proof that a session was completed.

## Implementation and test references

- `crates/platform/src/ipc/windows.rs` — per-user named pipe and DACL.
- `crates/platform/src/ipc/unix.rs` — XDG runtime/temp paths, lock, directory,
  and socket permissions.
- `crates/platform/src/ipc/mod.rs` — owner-before-database contract, message
  bounds, and forwarding timeouts.
- `crates/app/src/main.rs` — startup argument forwarding and owner-side callback
  dispatch.
- `crates/app/src/engine/tests.rs` —
  `cold_start_callback_with_no_pending_attempt_is_rejected` and completed
  session restoration cases.
- `crates/core/src/auth.rs` — `unsolicited_and_mismatched_callbacks_change_nothing`
  and the separate restore tests.
- `crates/platform/src/ipc/tests.rs` — private endpoint permissions, intact
  callback delivery, stale socket recovery, and concurrent owner election.
