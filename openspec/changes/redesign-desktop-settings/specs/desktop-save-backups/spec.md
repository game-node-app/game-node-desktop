# Spec Delta

## ADDED Requirements

### Requirement: Default retention for new games
Settings SHALL set the default number of ordinary versions kept per game (5, 10, 20 or 50; ten by default). Games without their own retention choice SHALL use the default; a game's own choice SHALL take precedence. Lowering the default SHALL NOT delete anything immediately; pruning SHALL happen at each game's next successful changed backup.

#### Scenario: Change the default
- **WHEN** the user sets the default to 20 and one game has its own retention of 5
- **THEN** other games keep up to 20 versions after their next backup, and that game keeps 5

#### Scenario: Lower the default
- **WHEN** the default changes from 20 to 5 for a game with twelve versions and no own choice
- **THEN** nothing is deleted until that game's next successful changed backup, which keeps the newest five

### Requirement: Backup space used per game
The application SHALL report the total space used by local backups and, per game with backups, its space and number of versions, recovery snapshots included. Computing it SHALL run off the UI thread and SHALL refresh after backups, restores and pruning.

#### Scenario: Space after a backup
- **WHEN** a 40 MB version is added for a game
- **THEN** that game's space and version count and the total increase accordingly

#### Scenario: Removed game
- **WHEN** a game is removed but its archives remain
- **THEN** its backups still count toward the total and appear under the game's last known title
