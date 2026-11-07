# Spec Delta

## Purpose

Give the user an opt-in, self-contained BitTorrent transfer surface inside GameNode: a versioned consent term, a bundled engine, a downloads list and detail, and a completion handoff, without any external client.

## ADDED Requirements

### Requirement: Opt-in consent with a versioned term
Downloads SHALL be off by default. Before any engine starts, the app SHALL show a consent term describing the bundled transfer engine, peer IP visibility, the user's responsibility for content, local disk use, and how to disable the feature. Accepting SHALL persist the term version and timestamp. Declining SHALL return to the library and leave the feature off. A newer term version SHALL require consent again.

#### Scenario: First visit to Downloads
- **WHEN** the user opens Downloads without a stored consent for the current term
- **THEN** the consent screen is shown instead of the transfer list
- **AND** no engine process starts and no listening port opens

#### Scenario: Add before consent
- **WHEN** the user tries to add a magnet or torrent file while consent is missing
- **THEN** the consent screen is shown first and no transfer is created

#### Scenario: Accept
- **WHEN** the user accepts the current term
- **THEN** the term version and timestamp are stored and the Downloads section becomes usable

#### Scenario: Decline
- **WHEN** the user declines the term
- **THEN** the library is shown, the feature stays off, and the next attempt shows the term again

#### Scenario: Updated term
- **WHEN** the stored term version is older than the bundled one
- **THEN** the consent screen is shown again before any transfer starts

### Requirement: Feature off means no engine process
While the feature is off, consent is missing, or the user has disabled it, the application SHALL NOT spawn the transfer engine, open a peer or API port, or contact peers. Disabling SHALL stop the engine after preserving resumable transfer state.

#### Scenario: Off at startup
- **WHEN** the application starts with Downloads disabled
- **THEN** no transfer engine process exists and no transfer port is listening

#### Scenario: Disable during a transfer
- **WHEN** the user disables Downloads while a transfer is downloading
- **THEN** the engine stops and the transfer keeps its resumable state for a later enable

### Requirement: Bundled transfer engine
The only transfer engine SHALL be the rqbit binary bundled with the application. The application SHALL NOT require, detect, configure or launch qBittorrent or any other external client. The active engine name and version SHALL be visible in Downloads and in Settings.

#### Scenario: Machine without a torrent client
- **WHEN** the user enables Downloads on a machine with no qBittorrent installed
- **THEN** adding and downloading a magnet works without any external client

#### Scenario: No qBittorrent surface
- **WHEN** Downloads and Settings are inspected
- **THEN** no qBittorrent connection, credentials, port or path setting exists

### Requirement: Engine lifecycle and recovery
The host SHALL start the engine on first use after consent, verify readiness before accepting transfers, and expose only an authenticated loopback API. A failed start SHALL be reported in the Downloads section. A crashed engine SHALL be restarted with backoff while transfers remain listed; repeated failure SHALL stop automatic restarts and offer a manual retry.

#### Scenario: Engine ready
- **WHEN** the user adds the first transfer
- **THEN** the engine starts, becomes ready, and the transfer appears

#### Scenario: Engine crashes
- **WHEN** the engine exits while a transfer is active
- **THEN** Downloads reports the interruption, restarts the engine, and the transfer resumes from its persisted state

#### Scenario: Repeated crash
- **WHEN** the engine crashes again before the backoff allows a stable restart
- **THEN** automatic restarts stop and the user is offered a manual retry

#### Scenario: Engine binary missing
- **WHEN** the bundled engine cannot start or is corrupt
- **THEN** Downloads shows a localized error with a retry action and no transfer is created

### Requirement: Downloads list
The Downloads section SHALL list every transfer known to the engine with its name, state, progress, total size, download and upload speed, ETA and peer count. It SHALL distinguish fetching metadata, downloading, paused, seeding, completed and failed. The list SHALL update while visible without blocking or replacing the library view.

#### Scenario: Fetching metadata
- **WHEN** a magnet is added and metadata has not arrived
- **THEN** the transfer shows a metadata state with peer progress instead of a percentage

#### Scenario: Active progress
- **WHEN** a transfer is downloading
- **THEN** its progress, speed, ETA and peers update while the section stays responsive

#### Scenario: Empty state
- **WHEN** the engine has no transfers
- **THEN** Downloads explains how to add a magnet or torrent file

#### Scenario: Engine unavailable
- **WHEN** the engine cannot be reached
- **THEN** Downloads shows the connection problem and keeps existing rows visible as unavailable

### Requirement: Adding a transfer
The application SHALL accept a magnet URI or a `.torrent` file supplied by the user. It SHALL validate the magnet info hash in hex or base32 form, reject malformed input with a localized message, and select the existing transfer when the same info hash is already managed.

#### Scenario: Valid magnet
- **WHEN** the user pastes a magnet with a valid `xt=urn:btih` value
- **THEN** a transfer is created and its metadata state is shown

#### Scenario: Torrent file
- **WHEN** the user chooses a valid `.torrent` file
- **THEN** a transfer is created from that file

#### Scenario: Malformed input
- **WHEN** the pasted text is not a magnet and no torrent file is chosen
- **THEN** a localized validation message is shown and nothing is added

#### Scenario: Duplicate info hash
- **WHEN** the user adds a magnet whose info hash is already managed
- **THEN** the existing transfer is selected instead of creating a duplicate

### Requirement: Transfer detail, files and destination
Selecting a transfer SHALL show its info hash, destination, total size, files and per-file size once metadata is available. The user SHALL be able to deselect files before or during download and choose the destination before any bytes are written. Changes SHALL be applied to the running transfer.

#### Scenario: Deselect a file
- **WHEN** metadata is available and the user deselects a file
- **THEN** the engine stops downloading that file and the remaining selection continues

#### Scenario: Choose destination
- **WHEN** the user selects a transfer before it starts writing
- **THEN** the destination can be changed and the chosen folder is used

#### Scenario: Destination locked
- **WHEN** a transfer has already written data
- **THEN** changing its destination is not offered and the reason is shown

### Requirement: Transfer actions and resumability
The user SHALL pause and resume transfers and forget a transfer while keeping its downloaded files. The application SHALL NOT offer deleting downloaded files in this change. Paused and interrupted transfers SHALL resume from persisted engine state after an application restart.

#### Scenario: Pause and resume
- **WHEN** the user pauses a downloading transfer and later resumes it
- **THEN** the transfer stops using the network and continues from its existing data

#### Scenario: Forget a transfer
- **WHEN** the user forgets a transfer
- **THEN** it leaves the engine and the Downloads list while its files remain on disk

#### Scenario: Restart with an active transfer
- **WHEN** the application restarts with a paused or interrupted transfer
- **THEN** the transfer returns with its previous progress and can be resumed

### Requirement: Completion handoff
A completed transfer SHALL offer exactly two primary actions: automatic installation and open destination folder. The completed state SHALL remain until the user acts on it or dismisses it. When an entry is already associated with the transfer, the handoff SHALL show that association instead of offering a duplicate addition.

#### Scenario: Completed transfer
- **WHEN** a transfer finishes downloading
- **THEN** it shows Instalação automática and Abrir pasta de destino as its primary actions

#### Scenario: Automatic install unavailable
- **WHEN** the transfer completes on a platform where automatic installation is not available
- **THEN** the install action is disabled with a localized explanation and Abrir pasta de destino remains available

#### Scenario: Open destination folder
- **WHEN** the user chooses Abrir pasta de destino
- **THEN** the operating system file manager opens the transfer's content folder and no library entry is created

#### Scenario: Already associated
- **WHEN** a completed transfer is already associated with a library entry
- **THEN** the handoff shows the associated entry instead of offering another addition

#### Scenario: Dismiss
- **WHEN** the user dismisses a completed transfer's handoff
- **THEN** the transfer stays completed and the folder action remains available from its detail

### Requirement: No content discovery or distribution
The application SHALL NOT provide torrent search, indexers, RSS, recommendations, curated sources or bundled content. It SHALL accept only a link or file the user supplies. It SHALL NOT send magnet URIs, torrent names or file lists to GameNode servers.

#### Scenario: No discovery surface
- **WHEN** Downloads is inspected
- **THEN** there is no search, browse, recommendation or source catalog

#### Scenario: No transfer metadata leaves the PC
- **WHEN** a transfer is added, progresses or completes
- **THEN** no magnet URI, torrent name or file list is sent to GameNode servers

### Requirement: Transfer errors and disk safety
Disk-full, permission, tracker, metadata-timeout and peer errors SHALL be reported on the transfer with a localized message and a retry or open-folder action where applicable. No error SHALL discard downloaded data or create a library entry.

#### Scenario: Disk full
- **WHEN** the disk cannot accept more data
- **THEN** the transfer reports the problem, pauses safely and keeps existing data

#### Scenario: Metadata timeout
- **WHEN** metadata cannot be obtained from peers or trackers
- **THEN** the transfer explains the timeout and offers retry or removal

#### Scenario: Permission denied
- **WHEN** the destination cannot be written
- **THEN** the transfer reports the path problem and does not create a library entry

### Requirement: Seeding follows an explicit policy
Completed transfers SHALL seed only according to the configured policy, and the user SHALL be able to pause seeding at any time. The default SHALL seed only while the application is running and stop when it quits.

#### Scenario: Quit stops seeding
- **WHEN** the user quits the application with a completed transfer seeding
- **THEN** the engine stops seeding with the application

#### Scenario: Pause seeding
- **WHEN** the user pauses a completed transfer
- **THEN** no upload traffic is produced for it while paused

### Requirement: Downloads UI is keyboard accessible and translated
Every Downloads control SHALL be reachable and operable by keyboard with visible focus. Every user-facing string SHALL be available in English and Brazilian Portuguese. Transfer names, paths and info hashes SHALL be shown as received.

#### Scenario: Keyboard operation
- **WHEN** the user navigates Downloads with the keyboard
- **THEN** they can add, pause, resume, open the folder and read the transfer state with visible focus

#### Scenario: Brazilian Portuguese
- **WHEN** the application runs with a Brazilian Portuguese locale
- **THEN** the consent term, list, detail, actions and errors are in Brazilian Portuguese and transfer names are unchanged
