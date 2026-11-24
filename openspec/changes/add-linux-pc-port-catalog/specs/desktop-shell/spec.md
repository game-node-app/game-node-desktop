# Spec Delta

## MODIFIED Requirements

### Requirement: Explore group in the sidebar
The signed-in sidebar SHALL show an `EXPLORE` group below `YOUR SPACE` containing the PC ports destination. Exactly one sidebar destination SHALL be active at a time. Selecting PC ports SHALL show the catalog in the content area, and selecting All games or a provider SHALL return to the library with that filter. The active destination SHALL NOT be restored after restart. The group SHALL be shown on Windows and Linux and SHALL NOT be shown on macOS or when signed out.

#### Scenario: Switch to PC ports
- **WHEN** the user selects PC ports in the sidebar
- **THEN** the content area shows the port catalog, PC ports is marked active, and All games and the providers are not

#### Scenario: Return to the library
- **WHEN** PC ports is active and the user selects Steam in the sidebar
- **THEN** the library is shown filtered to Steam and the library's previous view, sort and selection are preserved

#### Scenario: Restart
- **WHEN** the user quits while PC ports is active and starts the application again
- **THEN** it opens on the library

#### Scenario: Linux build
- **WHEN** a signed-in Linux build shows the sidebar
- **THEN** the `EXPLORE` group contains PC ports, and selecting it opens the catalog

#### Scenario: macOS or signed-out build
- **WHEN** the application runs on macOS, or no account is signed in
- **THEN** the sidebar shows no `EXPLORE` group

## ADDED Requirements

### Requirement: Windows navigation and filters remain unchanged
The Linux PC ports destination SHALL NOT change the existing Windows EXPLORE entry, Windows catalog filters, Windows install flow or Windows acceptance behavior.

#### Scenario: Windows build
- **WHEN** a signed-in Windows build opens PC ports
- **THEN** the existing Windows destination, filters and install actions are unchanged
