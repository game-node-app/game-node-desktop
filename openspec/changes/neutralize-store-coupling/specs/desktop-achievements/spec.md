# Spec Delta

## ADDED Requirements

### Requirement: Icon hosts come from the trusted plugin descriptor
The application SHALL fetch achievement icons only from the image hosts declared in the trusted descriptor of the plugin that provides those achievements. The host SHALL keep no store-specific host list of its own. A declaration SHALL be allowed only together with the achievements grant. Each entry SHALL be a lowercase DNS host name with at least two labels, and SHALL have no scheme, port, path, wildcard, or IP address. A descriptor SHALL declare at most 16 entries.

#### Scenario: Steam icon from a declared host
- **WHEN** a Steam achievement icon uses HTTPS on a host listed in the Steam descriptor
- **THEN** the icon is downloaded once, cached, and shown from the local cache

#### Scenario: Host declared by another provider
- **WHEN** a provider's achievement icon points to a host declared only by a different provider's descriptor
- **THEN** it is not fetched and a placeholder is shown

#### Scenario: Redirect to an undeclared host
- **WHEN** a declared icon host redirects to a host the same descriptor does not declare
- **THEN** the redirect is not followed and a placeholder is shown

#### Scenario: Invalid declaration
- **WHEN** a descriptor declares icon hosts without the achievements grant, declares more than 16 hosts, or declares a wildcard, IP address, port or path
- **THEN** the descriptor is rejected with a diagnostic at startup, and the other descriptors still load

#### Scenario: Icons cached by an earlier build
- **WHEN** a game's icons were cached before this change and its list is opened offline
- **THEN** the cached icons are still shown

### Requirement: Achievement messages name the provider
Achievement status messages SHALL name the entry's provider by its display name and SHALL NOT name a different store. This covers the privacy, not-connected and sign-in-again messages in the detail pane and the full list. The provider name SHALL be shown as declared, and the surrounding text SHALL be translated.

#### Scenario: Steam privacy failure
- **WHEN** Steam does not disclose an account's achievements
- **THEN** the detail pane and the full list explain that Steam's privacy settings prevent reading them

#### Scenario: Provider not connected
- **WHEN** a provider with an achievement backend is not connected and a game has no stored achievements
- **THEN** the detail pane says that connecting that provider, by its display name, shows achievements

#### Scenario: Translated message
- **WHEN** the application language is Brazilian Portuguese and the Steam session needs a new sign-in
- **THEN** the message is in Portuguese and names `Steam`
