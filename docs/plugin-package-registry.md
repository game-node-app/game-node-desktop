# Plugin package registry storage

The per-user plugin registry lives under `DataPaths::plugin_packages_dir()`. Each plugin key
and version key is encoded as hexadecimal path data, so package identity strings never become
filesystem path components. Version trees are create-once under `plugins/<id>/versions/<version>`;
the platform registry has no operation that edits or replaces an existing version. Candidate
`PluginPackageRegistry::install_signed` is the integrity-checked import path. It verifies the
detached Ed25519 signature and complete payload against the publisher key pinned by the user's
trust decision, copies the candidate into an inactive immutable version, verifies the copied
bytes again, and only then optionally activates it. A failed check leaves the previous active
pointer unchanged. `install_verified` remains a lower-level helper for already validated
candidates and tests; callers handling a signed `.gnplugin` should use `install_signed`.

An installation copies regular files and directories into a `package/` child of a unique staging
directory, rejects links and special filesystem objects, synchronizes the payload and writes a
registry completion record beside it. The record stays outside `package/`, so it is not an
unsigned extra file in the signed package tree. Only then does a same-filesystem directory rename publish the immutable version. Activation is
a separate atomic pointer replacement under `active/`. The previous pointer remains in place if
copying or publishing fails. A fully published candidate whose activation was interrupted stays
installed but inactive and can be explicitly activated later.

Opening the registry performs recovery: incomplete staging trees and abandoned temporary
pointer files are removed, complete versions are retained, and pointers to absent or incomplete
versions are removed. A pointer replacement can therefore resolve to the complete old or complete
new version after a crash, never a partial tree. Package extraction or registry calls are blocking
filesystem work and must run on a worker thread. The registry does not invoke candidate code,
installation hooks, or package-provided commands.

Immediately before process launch, the caller uses `verify_active_package` with the publisher key
from the trust store. This rereads the signed integrity metadata and hashes every payload file,
including the manifest, notices and executable, rejecting changed, missing, extra or linked files.
The key is supplied by the trust store; the package's self-declared key must not replace the
user-pinned identity during this check.

## Bundles

A bundle import does not create a second package registry. It stages the envelope, verifies the
outer signature and every component hash, and then installs each component through the normal
signed-package path. Two additive records group them:

- `plugin_bundles` stores the bundle id, name, version, publisher fingerprint, manifest hash and
  installation time;
- `plugin_bundle_components` maps the bundle to each component's plugin id, kind, package version
  and component hash.

Component packages, grants, activation and payloads stay in their existing tables and
directories. The mapping is written in the same transaction that records the staged component
installs; a failure removes staged files and leaves no mapping. Standalone imports remain
ungrouped. Uninstall resolves the mapping, stops every component with the normal bounded grace,
removes the packages and deletes the mapping; retained managed data follows the existing
persistence-retention rules.
