# Spec Delta

## MODIFIED Requirements

### Requirement: Automatic local-evidence matches are conservative
The first DRM-free matching pass SHALL automatically select a unique GameNode identity only when an evidenced external id maps uniquely, the port overlay names a GameNode game id for a port-installed entry, or a meaningful evidenced title matches uniquely after conservative normalization. It SHALL preserve sequel numbers and edition distinctions. Conflicting signals, ambiguous results, fuzzy-only similarity and incomplete lookup results SHALL remain unmatched suggestions.

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

#### Scenario: Port overlay identity
- **WHEN** a port install completes for an entry whose overlay names a GameNode game id
- **THEN** the entry is matched to that game automatically and its cover is requested, unless the user has already chosen a match

#### Scenario: Port without an overlay identity
- **WHEN** a port install completes for an entry with no GameNode game id in the overlay
- **THEN** the upstream game title is used as title evidence and the usual conservative rules decide whether to match
