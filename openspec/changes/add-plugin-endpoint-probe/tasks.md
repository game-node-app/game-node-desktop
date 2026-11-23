# Tasks

- [ ] 1.1 **Coverage record and conformance gate.** Keep `docs/plugin-endpoint-acceptance.md` as the
  source of truth with one row per endpoint family, the status legend and the homologation
  procedure. Extend `scripts/check-plugin-conformance.py` so every advertised route is either
  present in the page or listed in the documented deferral set, and fail with the missing route
  names. Verify the current page classifies all 85 advertised routes and both deferrals.

- [ ] 1.2 **Probe bundle skeleton.** Create `examples/plugins/rust-endpoint-probe` with one
  `provider` and one `extension` component, manifests declaring only the permissions each probe
  needs, dev publisher key instructions, MIT license and third-party notices. Package both
  components into one signed `.gnbundle` with `scripts/plugin-package.py bundle` and verify it with
  `bundle-verify`. Confirm the host import review, per-component consent and live enable/disable
  work for the development artifact, and that it is never distributed.

- [ ] 1.3 **Provider probes.** Implement probes for the control plane (`DescribeApi`,
  `GetPermissions`, `WatchPermissionChanges`), discovery, catalog, account, login, secrets,
  playtime counter, declared actions, achievements, library read, storage get/put/delete, data
  publish and files request/list/read. Each probe reports its exact status code and never swallows
  a denial.

- [ ] 1.4 **Extension probes.** Implement probes for standard launch and install, custom launch
  propose/request/remove, runtime list/get/watch/request-access/register/update/remove, runtime
  content registration, logical libraries and roots, files write/delete, network fetch, data
  read/watch, common categories, notifications, jobs, backups, save profiles, tools, ports,
  diagnostics, hardware, presence, performance, sessions and `ui.settings`.

- [ ] 1.5 **Scenario, report and control panel.** Read a JSON scenario of probe ids and
  expectations; write the report as a persistent `DataPublish` dataset plus a `StoragePut` summary
  and a stdout log. Add the Configure window with run, per-probe status and the grant revision
  before/after each interactive approval, with en/pt-BR labels and no UI-thread blocking. A probe
  that needs folder, origin, runtime or backup consent emits a notification and waits for the
  operator.

- [ ] 1.6 **Run the matrix and record it.** Execute the full sequence for every row on Linux:
  grant, use, revoke in flight, profile change, uninstall and reinstall. Record status and evidence
  in `docs/plugin-endpoint-acceptance.md`, report every defect found on a shared host path with its
  regression test, and leave a native Windows acceptance task
  open. Run `cargo test --locked -p gamenode-app`, the example suites,
  `python3 scripts/check-plugin-conformance.py`, `pnpm --dir ui test --run` and
  `openspec validate add-plugin-endpoint-probe --strict`.

## Out of Scope

- Replacing fixtures, automating native dialogs, new protocol methods or permissions, a marketplace
  and Windows native acceptance.
