# Spec Delta

## MODIFIED Requirements

### Requirement: Detect running games from install folders
While an account's library is open, installed store entries SHALL be running while a process executes inside their install folder. DRM-free entries SHALL instead require a configured or positively identified gameplay executable within their registered game folder; maintenance and launcher-only processes SHALL not count as gameplay. Paths SHALL follow OS case rules and folder boundaries. An executable SHALL be attributed to at most one entry, preferring an explicit binding and then the deepest folder.

#### Scenario: Game started outside GameNode
- **WHEN** the user starts an installed Steam game from Steam itself or from a desktop shortcut
- **THEN** GameNode marks that entry as running within one detection interval, without its window being open

#### Scenario: Similar folder names
- **WHEN** a process runs from `D:\Games\Portal 2 Mods\tool.exe` and an entry's install folder is `D:\Games\Portal 2`
- **THEN** that entry is not marked as running

#### Scenario: Several processes of one game
- **WHEN** a store game runs a launcher process and a game process from its install folder, and the launcher exits first
- **THEN** the entry stays running until the last of its processes exits

#### Scenario: External DRM-free start
- **WHEN** an added game's configured gameplay executable starts from a desktop shortcut
- **THEN** it is marked running and measured using source `manual` and platform 6 without requiring a GameNode match

#### Scenario: Uninstaller in the game folder
- **WHEN** only an uninstaller, crash reporter or updater runs inside a DRM-free game's folder
- **THEN** no gameplay session starts and no automatic save backup is triggered

#### Scenario: Duplicate executable ownership
- **WHEN** two registrations could claim the same gameplay process
- **THEN** deterministic ownership assigns it once and prevents duplicated playtime or exit-triggered backup

## ADDED Requirements

### Requirement: DRM-free launcher handoff preserves one play period
A known launcher SHALL be distinguished from gameplay. When a launcher hands off to a recognized game executable, tracking SHALL follow the game and SHALL not wait for an unrelated launcher or helper to exit. A bounded handoff interval SHALL join gameplay-process replacements within one play period. If no gameplay process is observed, the application SHALL not fabricate playtime or a successful game exit.

#### Scenario: Launcher persists
- **WHEN** a recognized launcher starts the configured game executable and remains open after the game exits
- **THEN** measured gameplay ends at the last gameplay-process exit and backup can proceed after settling

#### Scenario: Bootstrap exits before the game starts
- **WHEN** a bootstrap exits and the configured game starts within the handoff interval
- **THEN** one play period is tracked from the gameplay process's creation time, without an intermediate backup

#### Scenario: Unknown handoff
- **WHEN** a launcher starts an executable that cannot be attributed confidently to this installation
- **THEN** unrelated processes are not adopted and the entry offers tracking configuration when the window is shown

### Requirement: Physical game exit is distinct from session closure
The tracker SHALL expose a stable play-period identity and distinguish observed final gameplay exit from administrative session closure, checkpointing and 24-hour splitting. Only an observed stopped transition SHALL trigger an after-play backup. Repeated process notifications SHALL not create repeated exit events, and a failed process listing SHALL not be treated as an empty successful listing.

#### Scenario: Sign out during gameplay
- **WHEN** the user signs out while the game keeps running
- **THEN** the account's session closes under existing ownership rules without scheduling an after-play backup

#### Scenario: Duplicate exit callback
- **WHEN** an exit is reported by both a successful process listing and a process-handle notification
- **THEN** one physical-exit event is emitted for the play period

#### Scenario: Process listing fails
- **WHEN** Windows process enumeration fails while a game was running
- **THEN** the running period is not declared finished merely because that listing failed

### Requirement: Tracking continues through restart and inaccessible processes safely
DRM-free tracking SHALL retain the existing process identity, exit-wait and crash-checkpoint guarantees. A protected or uninspectable process SHALL not be guessed from its name. A recovered open session SHALL close at its checkpoint, and a still-running game SHALL begin a new observed period without causing a premature backup.

#### Scenario: GameNode restarts while the game runs
- **WHEN** GameNode restarts after a crash and observes the same game still running
- **THEN** prior playtime is recovered at the checkpoint, tracking resumes under the active account, and backup waits until the game actually exits
