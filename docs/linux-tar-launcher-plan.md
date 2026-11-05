# Experimental Linux tar launcher plan

Issue: [#2 — Linux tar.gz package with automatic integration (Arch + KDE)](https://github.com/game-node-app/game-node-desktop/issues/2)

Status: planned; implementation and package acceptance are pending.

## Goal and scope

Let users extract the Linux x86_64 tar package, run its launcher, and complete
browser sign-in without manually registering `app.gamenode://` or using `sudo`.
Initial acceptance targets Arch Linux with KDE Plasma on Wayland. Other Linux
environments remain experimental and unvalidated; a tar archive does not imply
distribution-independent library compatibility.

## Implementation

1. Add `packaging/linux/gamenode.sh` to locate its own directory, prepare and
   verify per-user protocol registration, then start the binary using `exec`
   and forward arguments with `"$@"` without shell interpretation.
2. Share registration logic with `packaging/linux/install.sh`. Preserve its
   stable-path installation option while allowing the launcher to register the
   extracted package in place. Respect XDG directories, escape desktop-entry
   paths correctly, and avoid unnecessary changes on subsequent launches.
3. Require `xdg-utils` and report missing prerequisites or registration failures
   before launching. Refuse conflicting entries or associations from another
   installation without silently replacing them; explain how to resolve them.
4. Adapt cleanup so it removes only the matching installation's integration,
   preserves application data, and does not undo subsequent user changes.
   Cleanup of an in-place package must not delete unrelated extracted files.
5. Update `scripts/package-linux.sh` to include the launcher and any shared
   helper. Update `packaging/linux/README.md` with prerequisites, the
   extract-and-run workflow, experimental support boundaries, and instructions
   to rerun the launcher after moving the extracted directory.

## Manual acceptance

Use a separate graphical test user or disposable VM. Do not rely on rollback of
the developer's real desktop configuration as the safety boundary.

- [ ] Extract the candidate tar package into a path containing spaces.
- [ ] Run the launcher without administrative privileges or manual XDG commands.
- [ ] Confirm that the application entry and protocol association reference the
      intended installation.
- [ ] Sign in through Firefox, accept the browser's request to open GameNode,
      and confirm that the application returns with an authenticated session.
      Use a browser profile without prior approval to exercise the prompt;
      direct return is also valid when permission was already granted.
- [ ] Run the launcher again and verify that sign-in still works.
- [ ] Move the extracted directory, rerun the launcher, and verify the updated
      association and callback.
- [ ] Check actionable errors for missing prerequisites and registration failure.
- [ ] Verify that another installation's integration is not silently replaced.
- [ ] Verify cleanup preserves user data and unrelated files or associations.

Record the package commit, results, and actual environment versions in issue #2
or its implementation PR. Reference environment from the initial manually
registered local-binary experience:

- Arch Linux; KDE Plasma 6.8.2; Wayland.
- Firefox 159.0; kernel `7.2.14-zen1-1-zen`.

That earlier experience does not constitute acceptance of the future package.

## Deferred

Podman, automated graphical infrastructure, and a multi-distribution test matrix
are not required for this delivery. Revisit automation once the working package
has a repeatable flow worth automating. Flatpak, AppImage, AUR, automatic updates,
and registration inside the application's sign-in flow are separate work.
