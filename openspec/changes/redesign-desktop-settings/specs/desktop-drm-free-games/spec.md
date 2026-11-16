# Spec Delta

## ADDED Requirements

### Requirement: Discovery preferences
Two preferences SHALL control watched-folder discovery, both on by default: "Check when GameNode starts" reconciles every watched folder at start, and "Watch for changes in real time" monitors watched folders for filesystem changes while the application runs. With either off, folders SHALL still be checked on demand ("Check all now", per-folder "Check") and on rescans. Turning real-time watching off SHALL stop monitoring immediately.

#### Scenario: No check at startup
- **WHEN** "Check when GameNode starts" is off and the application starts
- **THEN** no watched folder is traversed at start, and the last check time is unchanged

#### Scenario: Real-time watching off
- **WHEN** "Watch for changes in real time" is off and a game is copied into a watched folder
- **THEN** nothing is discovered until the user checks the folder or all folders

#### Scenario: Turn real-time watching back on
- **WHEN** the user turns real-time watching on again
- **THEN** monitoring resumes and missed changes are reconciled in the background

### Requirement: Watched folder status
For each watched folder the application SHALL report the number of library entries it holds, the time of its last completed check, and whether it is being checked, available or unavailable, with the time it became unavailable. A single folder SHALL be checkable on its own, off the UI thread. An unavailable folder SHALL keep its entries and count, and SHALL NOT be checked until it is reachable again.

#### Scenario: Folder count and last check
- **WHEN** a check of `D:\Jogos` completes with 18 entries
- **THEN** the folder reports 18 games and the completion time

#### Scenario: Check one folder
- **WHEN** the user checks `E:\Itch` while other folders are idle
- **THEN** only `E:\Itch` is traversed, it reports checking until done, and the others keep their state

#### Scenario: Drive comes back
- **WHEN** an unavailable folder's drive is reconnected
- **THEN** the folder becomes available and is reconciled in the background
