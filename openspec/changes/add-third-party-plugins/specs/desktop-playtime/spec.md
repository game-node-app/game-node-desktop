# Spec Delta

## MODIFIED Requirements

### Requirement: Detect running games from install folders
While an account's library is open, installed store entries SHALL be running while a process executes inside their install folder. DRM-free entries SHALL instead require a configured or positively identified gameplay executable within their registered game folder; maintenance and launcher-only processes SHALL not count as gameplay. Paths SHALL follow OS case rules and folder boundaries. For direct-executable games, an executable SHALL be attributed to at most one entry, preferring an explicit binding and then the deepest folder. Runtime-hosted games SHALL instead require validated content-session evidence; a shared executable path or PID SHALL NOT identify all hosted games as running.

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

### Requirement: Exits are observed, not polled
The application SHALL learn that a game process exited by waiting on that process, so the session ends at the time the process actually exited. A process that cannot be opened for waiting SHALL be treated as ended when it no longer appears in a process listing. Runtime content sessions SHALL additionally end or lose attribution at their validated content boundary independently of runtime process exit.

#### Scenario: Game exits between listings
- **WHEN** the last process of a running game exits
- **THEN** the entry stops being running and its session ends at that exit time, not at the next listing

#### Scenario: Process identity is reused
- **WHEN** a game process exits and the OS reuses its process id for an unrelated program
- **THEN** the unrelated program is not counted as the game

### Requirement: Measured sessions
For a provider without a store counter, or while the global Internal preference is selected, every validated play period SHALL be recorded as a session with a client-generated unique id, the game's library key, its matched GameNode game if any, the provider's playtime source, platform 6 and its start and end times. Direct-executable sessions SHALL retain first-process creation and last-process exit boundaries. Runtime content sessions SHALL use validated content start/end boundaries without inheriting an earlier runtime process creation time.

#### Scenario: Play an unmatched game
- **WHEN** the user plays a game that has no GameNode match
- **THEN** a session is recorded with its library key and no GameNode game
- **AND** it is kept until a later step can send it

#### Scenario: Start time before detection
- **WHEN** a game process was created 20 seconds before the next listing found it
- **THEN** the session starts at the process creation time

## ADDED Requirements

### Requirement: Runtime content attribution and deduplication
Core SHALL validate authorized plugin observations to distinguish content sessions and runtime instances independently of PID or executable path. Content switches SHALL close the prior interval at the validated boundary. Core SHALL arbitrate duplicate reports and process evidence into one accounting attribution per actual play period. Existing account, source, counter, checkpoint and platform policies SHALL remain authoritative.

#### Scenario: Content changes in the same process
- **WHEN** validated observations show game A ending and game B starting without restarting the emulator
- **THEN** A and B retain distinct intervals and histories without inheriting the runtime process creation time

#### Scenario: Concurrent runtime instances
- **WHEN** independently identified instances run different games using the same runtime executable
- **THEN** Core attributes their supported intervals independently

#### Scenario: Duplicate observations
- **WHEN** multiple plugins or process tracking report the same actual content play period
- **THEN** Core counts it once rather than adding duplicate playtime

#### Scenario: Launch fails before gameplay
- **WHEN** a launch names a game but successful gameplay is not confirmed
- **THEN** no runtime content playtime is fabricated from launch intent alone

#### Scenario: Platform tracking unavailable
- **WHEN** runtime observations arrive on a platform with unavailable gameplay tracking
- **THEN** the existing platform limitation remains and no measured session is recorded

### Requirement: Runtime uncertainty and common pause policy
Runtime accounting SHALL require bounded sequence/timestamps and fresh authorized evidence with a documented finite timeout. Lost, stale, conflicting or revoked identification SHALL stop extension at the last accepted observation and mark activity unknown. Fresh evidence SHALL NOT backfill gaps or infer content from a surviving PID. Confirmed pauses SHALL be excluded; absent pause detection SHALL follow the declared accounting mode and user preference.

#### Scenario: Emulator remains open without content
- **WHEN** confirmed content ends while the emulator stays open in its menu
- **THEN** the game interval ends and idle menu time is not charged to it

#### Scenario: Reporter disconnects or loses permission
- **WHEN** its producer disconnects, becomes stale or loses reporting authorization
- **THEN** confirmed time is preserved but the interval is not extended beyond its last accepted observation

#### Scenario: Restart or identification gap
- **WHEN** valid identification resumes after a gap or GameNode restart
- **THEN** a new supported interval starts without charging the unknown gap or reviving a stale PID binding

#### Scenario: Paused content
- **WHEN** the adapter reports pause and resume
- **THEN** Core excludes the confirmed paused interval from measured gameplay

#### Scenario: Descriptive presence only
- **WHEN** a plugin publishes character or map details without validated runtime-session evidence
- **THEN** presence alone neither starts measured gameplay nor proves safe backup or restore conditions

### Requirement: Declared playtime measurement policy
Plugins SHALL declare playtime and pause detection capabilities in their manifest. Without pause detection, the developer SHALL select application-open accounting or disabled measurement. Application-open accounting SHALL be labelled as such and require installation disclosure. Users SHALL be able to disable the plugin's accounting without disabling discovery or launching.

#### Scenario: Application-open accounting disclosure
- **WHEN** a plugin cannot identify pauses and declares application-open accounting
- **THEN** installation displays "Plugin incapaz de distinguir pausas no jogo, tempo de jogo aberto é contabilizado como tempo de jogo" in pt-BR, with equivalent localized wording elsewhere, and permits disabling accounting

#### Scenario: Developer disables unreliable accounting
- **WHEN** the manifest declares no playtime detection or selects disabled measurement
- **THEN** that plugin contributes no measured playtime and discovery and authorized launching remain available

#### Scenario: User disables accounting
- **WHEN** the user disables the plugin's accounting during installation or in settings
- **THEN** no new time is accrued from that plugin, existing history is preserved and independently authorized evidence from other sources remains subject to Core deduplication

#### Scenario: Runtime remains open without identified content
- **WHEN** application-open accounting is enabled but a shared runtime has no identified game or loses its content binding
- **THEN** elapsed runtime uptime is not assigned to unidentified games; freshness, content boundaries and unknown-gap rules still apply
