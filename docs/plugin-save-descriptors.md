# Future save descriptors

This roadmap describes optional metadata for a future negotiated capability. It is not a wire
schema, an implemented endpoint, or an instruction to install emulator tooling. The current API
supports save-profile configuration and approved-place inspection; profile-based backup execution
has separate unfinished work. A descriptor alone grants no file access, execution, sharing or
persistence. Unsupported save browsing/loading/import/conversion operations remain unavailable;
no successful stub endpoint is introduced by these examples.

The [illustrative fixture](fixtures/save-descriptor-roadmap.json) uses fictional games, producers,
formats and versions. Its ids are logical references, not usable resource handles or paths. Fields
show concepts to preserve when designing the later capability; names and shapes are not reserved
protobuf fields. `null` means unknown in this example. Absence also carries no evidence, and zero,
false or an empty string must not be substituted for an unknown fact. Future envelopes must define
bounds, schema versions and unknown-field preservation before exchanging these values.

## Artifact identity and format

Keep the game/content identity separate from the artifact, installation, save profile and runtime.
Several runtimes can use separate profiles for the same game without renaming old saves or losing
history. Distinguish `in_game_save` (including battery-backed data), `save_state` (a serialized
runtime session), `shared_container`, and `unknown`. A filename extension, size or directory name
alone cannot establish an artifact kind, format, compatibility or a platform slot.

A descriptor may report an attributed format id/version, exact or bounded expected byte sizes,
logical size observed, relative naming rules, supporting-file set and source runtime/core/version.
Size constraints belong to that format/version and must state their source; they are not universal
rules for every `.sav` or `.srm`. Missing format versions and size limits stay unknown. Do not
truncate, pad, rename or reinterpret original bytes merely to satisfy a suggested descriptor.

Content binding should identify the game plus the particular content variant: region, revision,
patch/mod identity, disc/set and a content fingerprint with its algorithm and observation provenance
where known. An unknown binding does not mean that all ROMs or versions are compatible. Re-evaluate
compatibility after content, runtime/core or format changes instead of relabelling historical
artifacts and backups to match the newly selected runtime.

## Compatibility evidence and shared resources

Compatibility is a producer-attributed claim with an observation time, evidence basis, source and
target format/runtime versions, content context and limitations. Outcomes include compatible,
conversion-required, incompatible and unknown. A claim about an in-game save cannot establish
save-state compatibility: a state can depend on an exact emulator/core version and content hash.
Core validates the eventual envelope and consent, not the producer's emulator knowledge.

A slot is optional platform-specific metadata, not necessarily an integer or a filename. A container
may be a single file, a directory set or a virtual memory card containing entries for several games.
Describe the selected entry separately from its backing container, sharing identity and affected
games. Selecting one game gives no authority to overwrite the whole card. Whole-container backup,
restore or conversion must review the complete affected-game scope and coordinate writes to the
backing resource; see [shared resource declarations](#shared-resource-declarations-and-consent).
Core need not parse console card layouts or infer slot semantics.

## Shared resource declarations and consent

A save profile may declare shared emulator resources through `shared_resources`. Each declaration
names a plugin-chosen `resource_id` alias, an approved folder handle and a display name. The host,
not the plugin, resolves the canonical backing folder and the complete affected GameNode
game/installation set from the standing folder approval. It rejects:

* a duplicate alias within one profile;
* two aliases for the same backing folder, or one alias for two backing folders, across the same
  plugin's profiles;
* nested or partially overlapping backing folders, because distinct lease keys would let one write
  bypass the other; and
* a declaration whose approved folder does not affect the profile's installation.

A shared resource also requires the profile's approved `runtime_id`; that binding gives the host a
runtime executable identity for external-process checks. A profile with shared resources cannot
change its runtime binding without saving the declarations again.

Consent is host-mediated and recorded separately from the plugin's profile row. It binds the
authenticated plugin, publisher, account profile, the exact declaration (alias, approval, relative
path, canonical backing folder and runtime binding) and a fingerprint of the host-resolved affected
installation/game set. Display-name or other descriptive edits do not invalidate it. Changing the
backing folder, runtime binding or affected set makes it stale and requires a fresh review; a
plugin RPC cannot accept, widen or replace it. `GetProfile`/`ListProfiles` report the state as
`PENDING`, `CURRENT` or `STALE` for the owner's profiles.

The host settings surface exposes `plugin_shared_resource_reviews`, `plugin_shared_resource_accept`
and `plugin_shared_resource_revoke`. The plugin settings screen lists each declared resource with
its host-resolved affected games and offers approve or revoke; it never receives a filesystem path.

Supervised runtime launches acquire a host-issued use lease for every reviewed resource attached to
the matching game installation and runtime. The lease is held until the supervised process tree is
confirmed exited or fenced. Backup and restore acquire one exclusive lease for the complete set
before starting the job, recheck the approved runtime's externally observed process state, and fail
closed when that state is unknown. Linux inspection reads `/proc` and compares the runtime's
executable identity, with a deleted or replaced image still attributed to its approved pathname;
unreadable process state fails closed. Windows uses the existing process inventory. Plugin-reported
game or session activity is descriptive and never establishes that a resource is idle.

## Load observations and images

`last_loaded` is an optional observed/reported event with source, time, game/content, runtime and
artifact attribution. File modification time, a recent backup, a launch request or a screenshot
cannot establish that an artifact was loaded successfully. An absent event stays unknown; it does
not mean never loaded. Replayed/stale reports must not silently replace newer evidence in a future
implementation. Created/modified timestamps retain their source and precision separately.

Images need provenance distinguishing a capture associated with a save from illustrative artwork.
Record producer, capture time where known, association basis and the logical image reference.
Artwork is not proof of gameplay, slot contents or a successful load. A future implementation must
use bounded approved byte/resource references, decode/resize images on workers, and separately
check file, sharing and retention grants. Descriptive metadata does not authorize capture or make
a thumbnail collector mandatory.

## Import and conversion lineage

Import preserves the original producer, format/runtime/content binding and source artifact identity;
the importer adds its own attribution rather than claiming to have produced the original. Conversion
creates a distinct output linked to the input and its fingerprint, converter identity/version,
source/target format and runtime, parameters understood by that converter, time, validation evidence
and known losses. Unknown output validation must remain unknown. A conversion claim is not an
instruction for Core to execute a tool or an arbitrary command embedded in metadata.

A future action flow must plan the output and affected resources, review independent permissions
and confirmation, preserve/back up originals, execute through an authorized plugin action, validate
the new output, and only then offer selection of its resulting profile. Failure or cancellation
preserves originals and provenance. Keep backup manifests and historical lineage attached to the
versions actually observed; switching runtime never rewrites them. File writes, backup/restore,
launching, sharing and durable retention each retain their own authority checks.

## Fixture reading guide and deferred work

| Fixture record | What it illustrates | What cannot be inferred |
| --- | --- | --- |
| `unclassified` | Unknown artifact kind, format, binding and last-loaded observation; unfamiliar versioned extension retained | Extension/size does not prove an in-game save or compatibility |
| `battery-save` | In-game save with fictional format/size constraints and attributed compatibility evidence | Compatibility with a different runtime/content variant or any save state |
| `runtime-state` | Runtime-specific save state, a reported load event and associated captured image | A general interchangeable in-game save format |
| `shared-card` | Shared backing container, named slots and multiple affected games | Permission to restore every entry from one game's consent |
| `converted-save` | Distinct output, input identity and converter/import provenance; validation unknown | Successful conversion, permission to execute a converter or permission to replace the input |

Save browsers, thumbnail/screenshot collectors, content collectors, platform parsers, memory-card
browsers, slot editors, importers, converters, migration jobs and universal save-loading endpoints
are deferred. No bundled emulator, converter executable or Core parser is required now. A later
capability needs version negotiation, bounded contracts, independent grants, action review and
interoperability/acceptance evidence before it can be advertised as supported. These fixtures are
documentary examples only; they neither parse real saves nor certify format support.
