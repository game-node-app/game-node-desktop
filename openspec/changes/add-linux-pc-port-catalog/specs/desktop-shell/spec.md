## ADDED Requirements

### Requirement: PC ports destination on Linux shows Coming Soon

A signed-in Linux build SHALL show the PC ports destination in the EXPLORE group with an
“Em breve”/“Coming Soon” label. Selecting it SHALL open the read-only catalog and SHALL NOT offer
an enabled install action. Signed-out builds keep the existing visibility rules.

#### Scenario: Linux signing-in state

- **WHEN** a Linux build is signed out
- **THEN** the PC ports destination remains hidden as it is today

#### Scenario: Signed-in Linux opens the destination

- **WHEN** a signed-in Linux build selects PC ports
- **THEN** the sidebar item and catalog header show Coming Soon, the catalog is browsable and no
  install action is enabled

### Requirement: Windows navigation and filters remain unchanged

The Linux PC ports destination SHALL NOT change the existing Windows EXPLORE entry, Windows
catalog filters, Windows install flow or Windows acceptance behavior.

#### Scenario: Windows build

- **WHEN** a signed-in Windows build opens PC ports
- **THEN** the existing Windows destination, filters and install actions are unchanged
