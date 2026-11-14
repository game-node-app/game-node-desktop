# Spec Delta

## ADDED Requirements

### Requirement: Store totals are summed per GameNode game
When several entries of one provider are matched to the same GameNode game, the application SHALL submit one store total for that game and source: the sum of those entries' counter totals. The once-per-change rule SHALL apply to the summed value. A single entry per game SHALL behave as before.

#### Scenario: Two Epic apps of one game
- **WHEN** two Epic apps that share a namespace are matched to the same GameNode game, with totals of 2 and 3 hours
- **THEN** one `epicgames` total of 5 hours is submitted for that game, and the two values never overwrite each other

#### Scenario: Unchanged sum
- **WHEN** a later read yields the same sum as the last accepted value
- **THEN** no request is sent

#### Scenario: Steam unchanged
- **WHEN** each Steam entry is matched to a different game
- **THEN** each total is submitted exactly as before this change
