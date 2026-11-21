# Third-party plugin manifest schema 1

Every `.gnplugin` package has a UTF-8 JSON `plugin.json` at its root. This document
describes the manifest accepted by the host. Acceptance validates declaration shape
and bounds only; it does not establish publisher trust, validate package signatures,
install the package, or execute plugin code.

## Required fields

| Field | Meaning |
| --- | --- |
| `schema` | Integer `1`. |
| `id` | Stable reverse-domain plugin identity. Official and reserved identity collisions are checked by host registration. |
| `kind` | `provider` or `extension`. |
| `provider_id` | Required for `provider`; forbidden for `extension`. Uses lowercase letters, digits, `-`, `_`. |
| `name`, `version` | Display name and SemVer plugin version. |
| `api` | Inclusive `min` and `max` SemVer API versions; minimum must not exceed maximum. |
| `targets` | Non-empty explicit OS/architecture/ABI tuples with package-relative `entrypoint`, optional `requires`, and informational `tested_on` environments. Targets are not inferred from a Cartesian product. |
| `publisher` | `key_id` and base64-encoded 32-byte Ed25519 public key. A valid key is not a trust decision. |
| `license` | SPDX expression or `LicenseRef-*`, plus one or more package-relative `files` and optional `notices`. |
| `schemas` | Optional versioned namespace descriptors and package-relative declarative schema asset paths. |

`permissions` is an optional list of `{id, required, scope, reason}` requests. Permission
requests do not grant access. `configuration` optionally declares `external` and/or
`declarative` configuration modes. `resources` optionally maps descriptor names to
package-relative resource paths. `source_url` is an optional absolute HTTP(S) URL;
`update_url` is an optional HTTPS URL.

`native_access` is an optional array of unique declarations from `files`, `network`,
`process`, `registry`, and `dbus`. These signed declarations disclose operating-system access
the program may use; they are not sandbox rules and do not grant GameNode plugin permissions.
Update review compares this list with the installed version.

The parser rejects unknown fields, duplicate JSON object keys at every depth, and SPDX
identifiers or exceptions absent from the strict SPDX catalog. Paths
must be relative, must not contain traversal or drive prefixes, and must not be empty.
The package importer is responsible for checking that referenced paths exist, are
listed in package integrity metadata, and have safe archive entries.

## Packaged extension schemas

`schemas` may contain at most 32 descriptors. Each descriptor requires `namespace`,
SemVer `version`, `classification` (`public`, `sensitive`, or `private`), and `asset`.
The namespace must equal the package plugin id or be its reverse-domain child; the
reserved `org.gamenode.*` namespace is unavailable. A package cannot declare the same
namespace/version pair twice, and schema asset paths must be unique under `schemas/`.
The package identity owns the namespace; an owner value is never read from the schema
payload. Package validation checks existing namespace claims from the host registry and
rejects exact or parent/child overlaps owned by a different plugin.

Schema asset bytes are supplied from the verified package payload to the manifest
validator; missing assets fail validation. Assets must be JSON and are limited to
64 KiB, nesting depth 8, and 256 total object keys. The accepted JSON Schema 2020-12
subset supports only `type`, `title`, `description`, `properties`, `required`,
`additionalProperties`, `items`, `enum`, numeric bounds, string/array size bounds, and
`x-gamenode-classification`. Object schemas must set `additionalProperties: false` and
declare `properties` and `required`; arrays require bounded `maxItems` and `items`, and
strings require bounded `maxLength`. `enum` is limited to 128 scalar values. Every
declared property carries `x-gamenode-classification`; the descriptor classification
must be at least as restrictive as the most restrictive field.

References (`$ref`, `$dynamicRef`, and related reference keywords), arbitrary keywords,
patterns, executable hooks, host-operation dispatch fields, and remote schema retrieval
are rejected. `$schema`, when present, must identify the supported dialect; it is never
used to fetch content. Reserved fields such as `publisher`, `provenance`, `revision`,
`permissions`, `command`, `operation`, `dispatch`, `action`, `executable`, and
`entrypoint` cannot be declared as extension properties. These schema assets describe
data shape and disclosure classification only; they do not add host behavior.

Example manifest descriptor and asset:

```json
{
  "schemas": [
    {
      "namespace": "org.example.reader.compatibility",
      "version": "1.0.0",
      "classification": "sensitive",
      "asset": "schemas/compatibility.v1.json"
    }
  ]
}
```

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "supports_cloud_sync": {
      "type": "boolean",
      "x-gamenode-classification": "public"
    },
    "notes": {
      "type": "string",
      "maxLength": 512,
      "x-gamenode-classification": "sensitive"
    }
  },
  "required": ["supports_cloud_sync"],
  "additionalProperties": false
}
```

## Target requirements and tested environments

Each target is an explicit tuple of `os`, `arch`, and `abi`. Schema 1 currently accepts
Linux (`gnu` or `musl`) and Windows (`msvc` or `gnu`) targets. Repeating a tuple is
rejected as an ambiguous selection. The `requires` object is optional and may contain
only the selector matching the target OS; unknown selectors and the other OS selector
are rejected.

For Windows, `requires.windows` can specify `product` (`client` or `server`),
`min_version`/`max_version`, and `min_build`/`max_build`. Version bounds are one to four
canonical decimal components compared numerically with missing components treated as
zero. Bounds are inclusive. At least one selector is required when the Windows object
is present.

For Linux, `requires.linux` can specify a libc family and minimum numeric version, a
distribution allowlist, or both. `glibc` requires the `gnu` target ABI and `musl`
requires `musl`. Distribution IDs are lowercase identifiers; each entry has one or more
exact version strings. Distribution entries are OR alternatives, as are the versions
within an entry. All other declared selectors apply together. Omitting the distribution
allowlist permits any distribution meeting the other requirements.

`tested_on` is separate informational author coverage. Windows entries have
`product` plus `release`; Linux entries have `distribution` plus `version`. They do not
add eligibility requirements or change how `requires` is evaluated.

### Target examples

The [example manifest](fixtures/plugin-target-compatibility.json) contains three targets
with illustrative requirements and placeholder publisher and payload values. The evaluator
tests load this file and supply fixed environment facts. Native acceptance exercises
[host detection](plugin-environment-detection.md) separately.

| Target | Example effective environment | Verdict |
| --- | --- | --- |
| Windows x86-64 MSVC, client, version at least `10.0`, builds `22000`–`26100` inclusive | Client build `22000` or `26100` with a matching version | Compatible |
| Same Windows target | Server at build `22631`, or client outside the build bounds | Incompatible |
| Linux x86-64 GNU, glibc `2.36` or newer, Debian `13` or `13.1` or Fedora `43` | Debian `13.1` with glibc `2.36` | Compatible even though author coverage only lists Debian `13` |
| Same glibc target | Debian `12`, a Debian-derived distribution whose own ID is absent from the allowlist, or glibc `2.35` | Incompatible |
| Linux x86-64 musl, minimum musl `1.2.4`, no distribution allowlist | Matching ABI and musl `1.2.4` with unknown distribution ID and release | Compatible |
| Same musl target | musl `1.2.3`, or a GNU userspace with glibc | Incompatible |
| Same musl target | musl family detected but version unknown, with the other required facts matching | Indeterminate |

Distribution allowlists match the effective userspace's `ID` and exact `VERSION_ID`.
For example, listing `"13"` does not include `"13.1"` unless both are declared.
`ID_LIKE` describes ancestry and never expands the allowlist. Compare libc versions
numerically component by component, with absent trailing components treated as zero.

The musl entry illustrates an interpreted plugin whose package ships an executable
launcher, a Python runtime and a native Python extension. Declare the OS, architecture,
ABI and minimum libc version required by the launcher, interpreter and loaded native
code. In this example the native extension requires musl `1.2.4`, so portable Python
source still needs a musl target with that minimum. Package the required files as
verified payload and test the actual runtime and dependencies as the author. Schema 1
evaluates the declared platform requirements and has no selectors for Python versions
or individual shared libraries. Compatibility checks leave runtime provisioning and
installation hooks outside the decision. The host evaluates environment facts without
starting the interpreter, installing dependencies or running package-supplied hooks.

A compatible verdict means every declared requirement passed. A known required
mismatch yields incompatible even when another required fact is unknown. If there
is no known mismatch and a required fact is unknown, the verdict is indeterminate.
Only a unique compatible target can be selected. A duplicate OS/architecture/ABI
tuple is rejected at manifest validation, even if its entrypoint and requirements
differ. The author test matrix records observed coverage, while the verdict describes
eligibility under the declared constraints. Successful launch depends on the actual
payload and runtime as well as the host's ordinary integrity, API and permission checks.

## ZIP staging limits

The platform extractor accepts an explicit payload allowlist for the selected
target and writes only to a new, private staging directory. It rejects paths
that escape the directory, duplicate and case-colliding entries, symbolic
links, unsupported special-file types, Windows device/reparse attributes, and
unlisted files. The selected target's declared payload must be present; a
different architecture's entrypoint cannot satisfy it. Limits are 256 MiB
compressed, 512 MiB expanded, 4,096 entries, and 4 MiB combined metadata.

The extractor is implemented in `gamenode_platform::plugin_packages` and runs
as a blocking platform operation; callers must invoke it on a worker. Package
validation must finish before this staging result can enter a registry or
process-launch path.

## Integrity signature bytes

`integrity.json` is a UTF-8 schema-1 object with a `files` object mapping each
payload-relative path to a lowercase, 64-character SHA-256 digest. It includes
the exact `plugin.json`, license, notice, schema and selected-target payload
bytes; it excludes `integrity.json` and `signature.ed25519`. Duplicate keys,
unknown top-level fields, unsafe paths, and malformed digests are rejected.

`signature.ed25519` is exactly 64 raw signature bytes. Verify Ed25519 over this
preimage, with no JSON parsing, reformatting, or newline insertion:

```text
ASCII("GameNode/.gnplugin/integrity/v1") || 0x00 || raw(integrity.json)
```

The implementation fixes this prefix in
`gamenode_platform::plugin_packages::SIGNATURE_DOMAIN_PREFIX`. It separates
GameNode plugin integrity signatures from signatures used for any other
message or product purpose. Rehash every payload from the immutable installed
version immediately before launch.

Managed package validation and update selection verify the signature and manifest, evaluate all
declared targets against the detected effective environment, and expose typed verdicts with the
author-tested declarations kept informational. Installation or activation requires exactly one
compatible target; incompatible and indeterminate candidates leave the active version and plugin
data untouched. Re-activating a stored version repeats this check against current environment
facts, and does not fall back to another version.

The process supervisor accepts host-owned `PackageIntegrity` configuration for managed
packages. Before creating launch credentials or spawning their executable, it revalidates the
signed directory, verifies that the executable resolves to its declared target inside that
directory, and reevaluates target requirements against refreshed effective-environment facts.
Unknown required facts and incompatible targets prevent startup; no fallback binary is tried.
Hashing and filesystem checks run on a blocking worker. Managed-package composition must
supply the pinned publisher key, immutable package root and explicit emulation policy.

## Building and checking a package

The repository tool `scripts/plugin-package.py` creates `.gnplugin` ZIP files, checks that
manifest-referenced license/resource/schema/entrypoint files exist, hashes every payload file,
signs the exact compact UTF-8 `integrity.json` bytes, and verifies the completed archive before
returning success. It does not execute package contents or installation hooks. It rejects
symlinks, special files, unsafe or case-colliding paths, payload changes, unlisted files and
archives beyond the documented size limits. The host remains authoritative for full manifest,
license, publisher-consent, target-compatibility and installation validation.

Install the pinned tool dependency and generate a test-only key:

```sh
python3 -m pip install -r scripts/plugin-package-requirements.txt
python3 scripts/plugin-package.py keygen --development --private-key ./fixture-dev.pem
```

The key-generation command prints the public key and fingerprint for the manifest's
`publisher` object, writes a `.development-key` marker beside the private key, and warns that
the generated key is for local fixtures only. When such a key signs a package, the tool includes
`DEVELOPMENT-KEY.txt` inside the signed archive. Never distribute that key or present the
resulting fingerprint as a production identity. Production signing keys must be provisioned and
protected by the publisher; this repository tool does not manage production key custody.

After adding the public key to `plugin.json`, build and verify without running plugin code:

```sh
python3 scripts/plugin-package.py pack \
  --source ./plugin-package \
  --private-key ./fixture-dev.pem \
  --output ./plugin.gnplugin
python3 scripts/plugin-package.py verify ./plugin.gnplugin
```

The `.gnplugin` file is a ZIP archive containing the source payload plus `integrity.json` and
raw `signature.ed25519`. `pack` refuses a key that does not match the manifest publisher key.
Deterministic Linux and Windows-shaped fixture round trips, tamper rejection, and inert hook
content are covered by `python3 -m unittest scripts.tests.test_plugin_package_tool`.

## Plugin bundles

A bundle distributes several managed packages as one signed download. It is a ZIP archive with a
manifest, existing `.gnplugin` components and one outer signature:

```text
bundle.json
components/provider.gnplugin
components/launch.gnplugin
signature.ed25519
DEVELOPMENT-KEY.txt        (only for bundles signed with a development key)
```

`bundle.json` is a schema-1 object:

```json
{
  "schema": 1,
  "id": "org.example.gamenode.dolphin",
  "name": "Dolphin GameCube and Wii",
  "version": "0.1.0",
  "publisher": { "key_id": "example-publisher", "public_key": "<base64 32-byte key>" },
  "components": [
    {
      "path": "components/provider.gnplugin",
      "sha256": "<lowercase SHA-256 of the exact component archive>",
      "plugin_id": "org.example.gamenode.dolphin",
      "kind": "provider"
    },
    {
      "path": "components/launch.gnplugin",
      "sha256": "<lowercase SHA-256 of the exact component archive>",
      "plugin_id": "org.example.gamenode.dolphin.launch",
      "kind": "extension"
    }
  ]
}
```

Each component remains a full signed package with its own `plugin.json`, `integrity.json` and
`signature.ed25519`. Every component's publisher public key and key id must equal the bundle's,
so one consent decision covers one publisher. Components keep their own plugin id, kind, API
range, permissions and lifecycle; the bundle never merges session kinds or shares grants.

`signature.ed25519` is 64 raw signature bytes over the exact `bundle.json` bytes, with no
re-serialization:

```text
ASCII("GameNode/plugin-bundle/v1") || 0x00 || raw(bundle.json)
```

Limits: 1 to 8 components, 512 MiB compressed bundle, 2 GiB expanded, no directory entries,
links, special files, nested bundles or paths outside `components/`. Duplicate plugin ids or
case-colliding component paths are rejected. Structural failures reject the whole import; a
component that is only platform- or API-incompatible is installed unavailable without blocking
its siblings.

Build and verify a bundle from already signed components:

```sh
python3 scripts/plugin-package.py bundle \
  --component ./provider.gnplugin \
  --component ./launch.gnplugin \
  --id org.example.gamenode.dolphin \
  --name "Dolphin GameCube and Wii" \
  --version 0.1.0 \
  --private-key ./fixture-dev.pem \
  --output ./dolphin.gnbundle
python3 scripts/plugin-package.py bundle-verify ./dolphin.gnbundle
```

The host still verifies each component with the normal package validator before installation.
The review pins one publisher fingerprint, shows the grouped per-component permissions, and
grants nothing. The inventory shows one bundle entry with per-component status; uninstall stops
and removes every component. Existing `.gnplugin` imports are unchanged.

## Example

```json
{
  "schema": 1,
  "id": "org.example.reader",
  "kind": "provider",
  "provider_id": "example-reader",
  "name": "Example Reader",
  "version": "1.2.3",
  "api": { "min": "1.6.0", "max": "1.7.0" },
  "permissions": [
    { "id": "library.read", "required": false, "scope": "own-provider", "reason": "Read imported games" }
  ],
  "native_access": ["files", "network"],
  "targets": [
    { "os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/linux-x64/plugin" }
  ],
  "publisher": { "key_id": "publisher-key-1", "public_key": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" },
  "license": { "expression": "MIT", "files": ["licenses/LICENSE"] },
  "configuration": { "external": true, "declarative": false },
  "resources": { "provider": "provider.toml" }
}
```

The manifest is consumed as data only. Package hashing/signature verification, publisher
fingerprint confirmation, schema ownership validation, target environment requirements,
and install/activation transactions are separate validation stages.
