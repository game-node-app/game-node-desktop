# Tasks

This change depends on `add-pc-port-catalog`. Do not archive it before that change is archived: its
requirements extend the same `desktop-port-catalog`, `desktop-port-install` and `desktop-shell`
capabilities. Windows behavior and the existing Windows acceptance are not reopened.

## 1. Phase 1 — Coming Soon catalog on Linux

- [ ] 1.1 Derive per-entry platform availability from the selected release's verified assets and
  the recipe's tool requirements, not from `supportedOS` alone. Expose native/blocked/unknown and
  the reason in the catalog view model. Verify Linux-only, Windows-only, mixed and inconsistent
  upstream claims (an OS listed without an asset) produce the expected availability.
- [ ] 1.2 Start the PC ports catalog service on signed-in Linux builds in read-only mode while the
  install service stays Windows-only. Keep the existing Windows catalog path unchanged.
- [ ] 1.3 Show `EXPLORE → PC ports` on signed-in Linux builds with an “Em breve”/“Coming Soon”
  label on the destination and in the catalog header. Verify signed-out and non-Linux builds keep
  their existing visibility.
- [ ] 1.4 Add the persisted platform filter **All** (default), **Native Linux** and
  **Windows-only** on Linux. Verify the filter survives restart, an unknown value falls back to
  All, and Windows filters are unchanged.
- [ ] 1.5 Keep every entry browsable on Linux in phase 1, but disable Install for all of them with
  the Coming Soon reason. The detail pane shows available platforms and the phase note. Verify no
  download, extractor, launch or library registration is started from a blocked card; Windows-only
  cards explain that Proton is future work.
- [ ] 1.6 Add en and pt-BR strings, `VITE_UI_MOCK=1` fixtures for native Linux, Windows-only and
  mixed entries, and component tests for the Coming Soon label, filter, blocked card and detail
  note.
- [ ] 1.7 Run a native Linux packaged-host check: open PC ports, see Coming Soon, list the cached
  catalog, filter native Linux and Windows-only, and confirm no install/network download starts.
  Record the evidence as a packaging check.

## 2. Phase 2 — native Linux installs

- [ ] 2.1 Select a native Linux release asset (AppImage, tar, zip or bare executable where the
  recipe allows it), verify its checksum and normalize the executable bit. Reject Windows-only
  assets and unsupported formats explicitly.
- [ ] 2.2 Map `app`, `data`, `config` and `localData` to XDG placement roots, register the result
  as an ordinary DRM-free entry, and validate the Linux launcher/executable before launch.
- [ ] 2.3 Support Linux guided setup only for overlay recipes explicitly reviewed for Linux. A
  recipe requiring the Windows PS4 PKG extractor is reported unavailable; never fetch or run that
  tool on Linux.
- [ ] 2.4 Extend the port job runner for Linux assets and placement: cancellation, crash recovery,
  no partial registration, journaled paths and no game-file deletion. Keep the Windows branch
  unchanged.
- [ ] 2.5 Remove the Coming Soon block for native Linux entries; keep Windows-only entries blocked
  with their own reason. Add en/pt-BR strings, mocks and tests for the mixed catalog.
- [ ] 2.6 Run focused Linux install fixtures and a packaged Linux acceptance with at least one
  native port built from the user's own files. Record the evidence separately from Windows
  acceptance.

## 3. Future

- Proton/Wine execution, Windows-only installs on Linux, macOS catalog/installs and a Linux PS4
  package extractor are future proposals. They are not tasks in this change.
