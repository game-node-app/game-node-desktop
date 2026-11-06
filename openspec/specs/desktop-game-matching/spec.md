# desktop-game-matching Specification

## Purpose

Link each detected game to the GameNode game it is, automatically by store id where GameNode knows the store and by the user's choice otherwise, so covers and GameNode actions work for it.

## Requirements

### Requirement: Automatic match by store id
For a provider whose manifest declares an external category, the application SHALL look up its detected ids with GameNode's existing external-game lookup, in batches within the server's limits. An id that maps to exactly one GameNode game SHALL be matched to it automatically. Steam uses category 1.

#### Scenario: Steam game known to GameNode
- **WHEN** a scan detects a Steam app id that GameNode maps to one game
- **THEN** the entry is matched to that game and shows its title and cover

#### Scenario: Large library
- **WHEN** a scan detects more Steam games than one lookup request accepts
- **THEN** all of them are looked up across several requests

#### Scenario: Store id maps to several games
- **WHEN** the lookup returns more than one GameNode game for one store id
- **THEN** the entry is not matched automatically and the match dialog lists those games first

### Requirement: Search for unmatched games
An entry without a match SHALL offer a match dialog that searches GameNode's existing game search, prefilled with the detected title. Results SHALL show the title, release year and cover. The user SHALL be able to edit the query and pick a result.

#### Scenario: Match by hand
- **WHEN** the user opens Match on an unmatched game and picks a search result
- **THEN** the entry is matched to that game and its cover is downloaded

#### Scenario: Search fails
- **WHEN** the search request fails or times out
- **THEN** the dialog shows a localized error and the entry keeps its current state

### Requirement: Change or clear a match
The user SHALL be able to change any entry's match, automatic or chosen, and to mark an entry as having no GameNode game. The detail pane SHALL show whether the current match was automatic or chosen.

#### Scenario: Replace a wrong automatic match
- **WHEN** the user picks another game for an automatically matched entry
- **THEN** the entry uses the chosen game from then on

#### Scenario: Mark as not on GameNode
- **WHEN** the user marks an entry as having no GameNode game
- **THEN** the entry stays unmatched, playable, and is not matched automatically again

### Requirement: The user's choice wins
A match or no-match chosen by the user SHALL NOT be replaced by a later automatic lookup. Automatic matches without a user choice SHALL be refreshed by later lookups. A failed lookup SHALL NOT change any match.

#### Scenario: Rescan after a manual pick
- **WHEN** the user picked a match and a later lookup returns a different game for that store id
- **THEN** the user's pick is kept

#### Scenario: Lookup fails
- **WHEN** a lookup request fails during a scan
- **THEN** existing matches are kept and the next scan tries again

### Requirement: Matches belong to the account
Matches chosen by the user SHALL be stored per GameNode account. Another account signed in on the same PC SHALL NOT see or use them, and SHALL get its own automatic matches.

#### Scenario: Another account signs in
- **WHEN** user A picked matches, signs out, and user B signs in on the same PC
- **THEN** B's library has none of A's chosen matches

#### Scenario: Same account signs in again
- **WHEN** user A signs out and later signs in again on the same PC
- **THEN** A's chosen matches are restored

### Requirement: Matching needs a verified session
Lookups and searches SHALL be sent only with a verified session. Without one, entries SHALL keep their stored matches, and the match actions SHALL explain why they are unavailable.

#### Scenario: Offline start with a cached library
- **WHEN** the application opens the cached library while GameNode is unreachable
- **THEN** stored matches and covers are shown, Match is disabled with an explanation, and lookups resume after the session is verified

### Requirement: Infer a title before asking for a match
For DRM-free entries, the application SHALL attempt title inference from executable product metadata, meaningful executable names and installation context, enriched by manifest evidence when available. Generic engine, launcher and maintenance names SHALL not establish an identity. It SHALL retain the detected title separately from the catalog title and allow the user to edit it.

#### Scenario: Product metadata identifies the game
- **WHEN** `game.exe` has a meaningful product title supported by its installation context
- **THEN** that title is used for the first catalog lookup without asking the user to type it

#### Scenario: Useful executable name
- **WHEN** `Celeste.exe` has no useful product metadata
- **THEN** its meaningful filename is tried as title evidence and the entry remains playable during lookup

#### Scenario: Generic binary name
- **WHEN** a binary is named `Game-Win64-Shipping.exe` and its folder gives no reliable game identity
- **THEN** the name does not cause a speculative automatic match and manual search remains available

### Requirement: Automatic local-evidence matches are conservative
The first DRM-free matching pass SHALL automatically select a unique GameNode identity only when an evidenced external id maps uniquely or a meaningful evidenced title matches uniquely after conservative normalization. It SHALL preserve sequel numbers and edition distinctions. Conflicting signals, ambiguous results, fuzzy-only similarity and incomplete lookup results SHALL remain unmatched suggestions.

#### Scenario: Manifest external identity
- **WHEN** a strongly identified manifest game has a Steam id that GameNode maps to one game
- **THEN** that identity can match the manual entry without changing its provider, playtime source or launch behavior

#### Scenario: Unique exact title
- **WHEN** executable evidence gives `Celeste` and a completed search returns one exact title match
- **THEN** the entry is matched automatically and its cover is requested

#### Scenario: Edition ambiguity
- **WHEN** the evidence does not distinguish an original game from a remaster, or a sequel from its predecessor
- **THEN** no automatic match is chosen and the user can select a suggested result

#### Scenario: Several exact results or truncated search
- **WHEN** several GameNode identities share the evidenced title or search completeness cannot be established
- **THEN** the application does not select the first returned result automatically

### Requirement: Local matching honors choices and stale-result boundaries
Local-evidence matching SHALL obey existing account-specific chosen-match and no-match overrides. It SHALL retain valid cached matches on lookup failure and ignore results for superseded executable evidence, removed entries or previous accounts. Match provenance and a way to change it SHALL be visible in details.

#### Scenario: Wrong first attempt
- **WHEN** an automatic match is wrong and the user selects another game with Change match
- **THEN** subsequent title inference, rescans and manifest updates retain that choice

#### Scenario: Explicit no-match
- **WHEN** the user marks the entry as having no GameNode game
- **THEN** automatic matching does not resume unless that choice is cleared

#### Scenario: Slow lookup after repair
- **WHEN** an executable is repaired while an earlier lookup is still running
- **THEN** its old result cannot replace the repaired entry's current match
