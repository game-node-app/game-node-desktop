# DRM-free games and local saves

On Windows, choose **Add game** in the library toolbar or empty library, then select
the game's executable. Cancel leaves the library unchanged. The local entry is
playable immediately, including offline; a GameNode match is optional. Adding the
same canonical executable selects its existing entry. Separate installations keep
separate identities, settings, sessions and archive namespaces.

**Game settings** edits the detected title, executable, game folder, working
directory and argument list. **Locate executable** repairs a moved installation
without changing its identity. **Remove from library** removes only the registration:
installed files, live saves, measured sessions and archives remain on disk. A missing
executable stays visible. Store entries continue to use their own launch/install rules.
Community native PC ports installed from **PC ports** become the same kind of DRM-free
entry; see [pc-ports.md](pc-ports.md) for the catalog, overlay and install rules.

## Identity and corrections

Identification reads bounded PE product/description metadata without executing the
file, then meaningful executable names and installation context. Generic engine,
launcher, installer and maintenance names do not establish a title. Manifest aliases
and installation/launch hints can corroborate executable evidence; a folder name
alone does not prove that a program is a game. Edition and sequel distinctions are
preserved. Automatic matching requires a unique external identity or a unique exact
result from a complete catalog search. Ambiguous or incomplete results stay suggestions.

Use **Change match** to correct the catalog identity, or explicitly mark that no
GameNode game exists. These account-specific choices override automatic inference.
Catalog corrections do not change the independent save profile or retarget old backups.
Cached identity remains available offline; stale responses after repair, removal or
account changes cannot overwrite the new state.

## Tracking

Tracking samples processes every five seconds and uses PID plus creation time to
avoid PID reuse. DRM-free gameplay requires an explicit executable binding inside
the registered folder. Store folder matching remains supported. Launcher and
maintenance processes alone do not count as gameplay. A ten-second handoff window
joins observed configured gameplay replacements; its settling time is excluded from
the game's final exit timestamp. Backup then waits three more seconds and checks
again that gameplay is stopped.

### Supported handoff behavior
- **Persistent launcher:** When a configured launcher (e.g., `launcher.exe`) spawns the
  actual game process (e.g., `bin/game.exe`), tracking automatically recognizes and learns
  the child process if it is within the game root and shares the game's identity. Once the
  gameplay process exits, gameplay tracking stops after the 10-second handoff settling period,
  even if the launcher remains open in the background.
- **Bootstrap gap:** If an initial bootstrap process exits shortly before the main game process
  starts, the ten-second handoff bridges the gap into a single uninterrupted play period.
- **Learned gameplay persistence:** Once a gameplay process is positively learned from a launcher,
  it is saved into the game's registration. Subsequent external starts directly from shortcuts
  track the learned executable immediately without requiring launcher intervention.

### Known limitations and recovery
- **Polling interval:** Because process enumeration occurs every five seconds, short-lived helper
  or benchmark runs lasting under 5 seconds may exit unobserved.
- **Elevated / uninspectable processes:** Games running with elevated administrator privileges or
  protected by certain kernel anti-cheat systems may deny process query access to standard-user
  GameNode instances. In such cases, GameNode reports the process as uninspectable rather than guessing.
- **Outside-root executables:** If a game launcher launches an executable located outside the
  registered game root (for example, in `%PROGRAMDATA%` or a shared engine installation), GameNode
  cannot adopt it automatically. To track these games, open **Game settings** and configure the game
  root to a common parent folder or add the target executable directly.
- **Edit-tracking recovery:** If a game was added with an initial target that is not tracked, or if
  additional executables are needed, open **Game settings** and edit the gameplay and launcher lists.
  Saving updates the tracking policies immediately in memory and in the local database, allowing active
  matching without resetting archive history or save profiles. Sign-out, source changes, session
  checkpoint/splitting and application shutdown do not fabricate exits. Measured DRM-free sessions
  remain local, with source `manual` and platform 6.

## Watched folders and review

Open **Local save protection**, enter an opt-in library folder such as
`F:\NonSteamLibrary`, and choose **Watch folder**. **Rescan watched folders** reconciles
missed notifications; event bursts are debounced. Discovery is bounded to selected
roots, eight levels and 10,000 entries per installation. It never runs an executable.
Unknown or installer-only folders are omitted rather than guessed from extensions.

Select review candidates and choose **Add selected** or **Ignore selected**. A
candidate with several targets requires executable correction first. Candidates have
no library, tracking or backup effect before approval. Ignored fingerprints survive
restart; incidental files do not change them. **Remove watch** stops monitoring and
retains registrations. An unavailable drive retains registrations and decisions.

## Save profiles, backups and restore

**Local save protection** shows support independently of the catalog match. Missing
support does not block Play. Select **Choose save file** or **Choose save folder** for
a custom location, or enter an exact manifest title with **Change save profile**.
A scoped preview determines locations before activation. An empty current save state
is valid. Explicit retargeting keeps history under the previous profile identity and
cancels queued jobs for obsolete rules.

Automatic local protection defaults on for supported profiles, with global and
per-game controls. It works with GameNode sync off and never uploads save bytes.
**Backup now** uses the same stopped-game gate. The default destination is
`%LOCALAPPDATA%\GameNode\Desktop\backups`. A destination must not overlap live saves,
registered games or watched roots, including aliases. Changing destination affects
new jobs and retains old namespaces for history.

The bundled Ludusavi v0.31.0 helper uses a private config and one selected game at a
time. Its executable is packaged with pinned digests; it is never downloaded at
runtime. The pinned baseline manifest supports offline startup. Data-only manifest
updates use the fixed upstream HTTPS source, daily ETag checks and a validated cache;
jobs retain their selected rule snapshot. Game-local secondary manifests cannot
silently widen the save scope.

Archives live at `backups/manual/<installation UUID>/<profile ID>/`. Ten successful
changed full versions and zero differential versions are retained. Unchanged data
adds no version. Partial, cancelled or failed staging does not replace usable active
history. Transient jobs retry after 30 seconds, two minutes and ten minutes. Running,
queued, unchanged, partial and failed outcomes must remain distinguishable.

Choose **Load backup history**, select a version and review its file/registry targets.
Only **Confirm restore** authorizes replacement. GameNode checks that the game is
stopped, preserves the current affected data in a verified recovery namespace, checks
again and restores the exact selected version. Recovery failure aborts replacement.
Unrelated files remain untouched. External starts during preparation cancel work;
coordination with other launchers is best effort. Partial restore retains its recovery
record at `.gamenode-restore-recovery.json` in the profile namespace.

Recovery archives are outside ordinary ten-version retention. To reclaim their space,
quit GameNode, inspect the recorded recovery path and remove only recovery data you
have explicitly decided to discard. Uninstall preserves user data and archives.
A backup on the same physical drive protects against save corruption and accidental
overwrite, but cannot protect against loss of that drive.
