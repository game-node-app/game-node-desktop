# Pinned backup release inputs

Run `python3 scripts/prepare-backup-assets.py <output-directory>` to prepare
Windows backup assets. `backup-inputs.json` is the release source of truth:
Ludusavi v0.31.0 Windows x64 and a primary manifest at an immutable Git revision.
Every downloaded input is SHA-256 checked before any output is published.
`build-inputs.json` records both the release contract and extracted asset digests.
The preparation directory is build output, not application runtime configuration.

The upstream executable and its license come from mtkennerly/ludusavi, tag
v0.31.0. The dataset and its license come from mtkennerly/ludusavi-manifest at
the recorded revision. Preserve both license files in distributed packages.
The primary dataset aggregates game-save definitions maintained by that project;
its individual entries are metadata, not executable commands. Source URLs and
license hashes are recorded alongside the manifest hash in the release contract.

The pinned CLI help is also verified as a release input. The adapter contract
requires isolated `--config`, `--no-manifest-update`, backup/restore
`--no-cloud-sync`, `--preview`, JSON `--api`, selected restore `--backup`, and
`--full-limit` / `--differential-limit`. Always scope operations to one title.
These release inputs do not activate backup support by themselves; package and
adapter wiring are tracked separately in the OpenSpec change.
