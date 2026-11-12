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

```json
{
  "targets": [
    {
      "os": "windows",
      "arch": "x86_64",
      "abi": "msvc",
      "entrypoint": "bin/windows-x64/plugin.exe",
      "requires": {
        "windows": {
          "product": "client",
          "min_version": "10.0",
          "min_build": 22000
        }
      },
      "tested_on": [{ "product": "windows-11", "release": "23H2" }]
    },
    {
      "os": "linux",
      "arch": "x86_64",
      "abi": "gnu",
      "entrypoint": "bin/linux-x64/plugin",
      "requires": {
        "linux": {
          "libc": { "family": "glibc", "min_version": "2.36" },
          "distributions": [
            { "id": "debian", "versions": ["13"] },
            { "id": "fedora", "versions": ["43"] }
          ]
        }
      },
      "tested_on": [{ "distribution": "debian", "version": "13" }]
    }
  ]
}
```

This schema validation does not detect the current execution environment or decide
compatibility. That host-side detection and requirement evaluation are separate stages.

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

The process supervisor accepts host-owned `PackageIntegrity` configuration for managed
packages. Before creating launch credentials or spawning their executable, it revalidates the
signed directory and verifies that the executable resolves inside that directory. Hashing and
filesystem checks run on a blocking worker. Failed verification prevents process startup;
managed-package composition must supply the pinned publisher key and immutable package root.

## Example

```json
{
  "schema": 1,
  "id": "org.example.reader",
  "kind": "provider",
  "provider_id": "example-reader",
  "name": "Example Reader",
  "version": "1.2.3",
  "api": { "min": "1.4.0", "max": "1.5.0" },
  "permissions": [
    { "id": "library.read", "required": false, "scope": "own-provider", "reason": "Read imported games" }
  ],
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
