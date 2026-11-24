# Spec Delta

## MODIFIED Requirements

### Requirement: Public pool permission controls
Installation and plugin settings SHALL offer independent publication, read, create, update and delete permission choices for the local public pool. The user SHALL be able to select category-wide scopes or individual records. All grants SHALL remain limited to requested, supported operations; persistence SHALL require its separate permission. The settings SHALL show durable-pool usage and retention per producer without exposing physical paths.

#### Scenario: Read category and update selected record
- **WHEN** the user grants category-wide read and update on one published runtime record
- **THEN** the plugin can read that category and update that record but cannot update other records or delete any record without another explicit grant

#### Scenario: Durable usage is visible
- **WHEN** a producer has durable shared records
- **THEN** its settings show the retained size, record count and retention limit, and the user can delete the retained data without deleting the plugin
