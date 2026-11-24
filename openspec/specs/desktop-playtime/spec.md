# desktop-playtime Specification

## Purpose

Record how long the user plays each detected game, whichever way it was started, and turn that into GameNode playtime: store totals where a provider exposes its own counter, measured sessions kept in a local queue everywhere else.

## Requirements

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

### Requirement: Tracking belongs to the signed-in account
The application SHALL track only while an account's library is open, including a cached library whose session is not yet verified. Every recorded session SHALL belong to that account. Signing out, or a rejected session, SHALL first end any open session at that moment. No account SHALL see, change or send another account's sessions.

#### Scenario: Sign out during play
- **WHEN** the user signs out while a game is running
- **THEN** the open session ends at the sign-out time and stays in the first account's queue
- **AND** tracking stops until an account's library is open again

#### Scenario: Another account signs in
- **WHEN** a different GameNode account signs in on the same PC
- **THEN** it starts with its own empty queue and the first account's sessions are kept but not visible to it

#### Scenario: Signed out
- **WHEN** no account's library is open
- **THEN** no processes are tracked and no sessions are recorded

### Requirement: Measured sessions
For a provider without a store counter, or while the global Internal preference is selected, every validated play period SHALL be recorded as a session with a client-generated unique id, the game's library key, its matched GameNode game if any, the provider's playtime source, platform 6 and its start and end times. Direct-executable sessions SHALL retain first-process creation and last-process exit boundaries. Runtime content sessions SHALL use validated content start/end boundaries without inheriting an earlier runtime process creation time.

#### Scenario: Play an unmatched game
- **WHEN** the user plays a game that has no GameNode match
- **THEN** a session is recorded with its library key and no GameNode game
- **AND** it is kept until a later step can send it

#### Scenario: Start time before detection
- **WHEN** a game process was created 20 seconds before the next listing found it
- **THEN** the session starts at the process creation time

### Requirement: Sessions survive crashes
An open session SHALL be written to local storage when it starts and checkpointed at least once a minute while it runs. After the application stops unexpectedly, its open sessions SHALL be closed at their last checkpoint the next time the application starts.

#### Scenario: Power loss during play
- **WHEN** the PC loses power while a game session is open and GameNode starts again later
- **THEN** that session ends at its last checkpoint and is kept in the queue
- **AND** the time after the last checkpoint is not counted

#### Scenario: Game still running after a GameNode restart
- **WHEN** GameNode stops unexpectedly while a game keeps running and GameNode is started again
- **THEN** the old session is closed at its last checkpoint and a new session starts for the running game

### Requirement: Long sessions are split
No recorded session SHALL last longer than 24 hours. A session that reaches 24 hours SHALL be closed and a new one SHALL start at the same instant for the same game.

#### Scenario: Game left running for 30 hours
- **WHEN** a game runs for 30 hours
- **THEN** the queue holds a 24-hour session followed by a 6-hour session with no gap or overlap

### Requirement: The session queue is not sent in this step
Recorded sessions SHALL stay in the local queue, in order, until a sessions endpoint exists. This change SHALL NOT send measured sessions to GameNode or remove them from the queue.

#### Scenario: Sessions after reconnecting
- **WHEN** sessions are recorded and GameNode becomes reachable
- **THEN** the sessions remain in the local queue and no session request is made

### Requirement: Providers with a store counter submit totals
While the global Store preference is selected, for a provider that declares a store counter, the application SHALL NOT record measured sessions. It SHALL read the store's total for a game after that game stops running, when the application starts and after each scan, and SHALL submit it to GameNode as that game's total for the provider's source and platform 6.

#### Scenario: Steam game exits
- **WHEN** a matched Steam game exits, Steam's counter is declared, and sync is on
- **THEN** GameNode reads Steam's total for that game and submits it as the `steam` total for platform 6
- **AND** no measured session is recorded for that play

#### Scenario: Play from before installing GameNode
- **WHEN** GameNode starts for the first time and Steam's counter already holds hours for a matched game
- **THEN** that total is submitted, including the play from before GameNode was installed

### Requirement: Totals are submitted once per change
The application SHALL remember, per account and game, the last total GameNode accepted, and SHALL submit a total only when it differs from that value. A total SHALL be held while its game is unmatched, while sync is off or while the session is unverified or GameNode is unreachable, and submitted at the next opportunity once that changes.

#### Scenario: Unchanged total
- **WHEN** a scan reads a Steam total equal to the last accepted one
- **THEN** no request is sent

#### Scenario: Offline exit
- **WHEN** a Steam game exits while GameNode is unreachable
- **THEN** its total is submitted after GameNode becomes reachable, at the next start, scan or game exit

#### Scenario: Game matched later
- **WHEN** an unmatched Steam game with counted play is matched by the user
- **THEN** its total is submitted for the chosen GameNode game

### Requirement: Tracking is Windows-only in the MVP
Process tracking SHALL run on Windows. On other platforms the application SHALL report tracking as unavailable, record no sessions and continue to work otherwise.

#### Scenario: Running on Linux
- **WHEN** the application runs on Linux
- **THEN** no entry is ever shown as running, no session is recorded, and the library works as before

### Requirement: Global playtime source preference
Settings SHALL offer a PC-wide Store (default) or Internal preference. Store SHALL use supported provider counters; games without one SHALL always use Internal. Internal SHALL measure sessions and prevent store-total submissions. Switching SHALL apply immediately and preserve histories separately without merging imported totals into session durations.

#### Scenario: Default Steam source
- **WHEN** a user plays a Steam game with default preferences
- **THEN** Steam's store total is imported and no measured session is created

#### Scenario: Internal selected globally
- **WHEN** the user selects Internal and plays Steam and non-Steam games
- **THEN** all games use measured sessions and no store-total request is sent

#### Scenario: No store counter
- **WHEN** Store is selected and a game has no supported store counter
- **THEN** that game uses measured sessions

#### Scenario: Switching during play
- **WHEN** the user changes the preference while a game is running
- **THEN** the source changes immediately without merging histories or counting earlier imported time as a new internal session

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

### Requirement: Last played reflects the selected source
The library SHALL expose each entry's last-played time from the selected playtime source. With Store selected it SHALL come from that game's store total. With Internal selected it SHALL be the end of the entry's most recent measured session, including the current session while the game runs. When the source has no date, last played SHALL be unknown rather than invented. Account changes SHALL clear it before the new account's values load.

#### Scenario: Store last played
- **WHEN** Store is selected and the store reports a last-played date for a game
- **THEN** that date is used for the entry's Last played sort and filter

#### Scenario: Internal last played
- **WHEN** Internal is selected and a game's latest session ended at 18:00
- **THEN** last played is 18:00, and it follows the current session while the game runs

#### Scenario: No date
- **WHEN** the selected source has no last-played date for an entry
- **THEN** last played is unknown, the entry sorts last for that key, and no last-played window filter counts it

#### Scenario: Account change
- **WHEN** a different account signs in
- **THEN** the previous account's last-played dates are cleared before the new values load

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
