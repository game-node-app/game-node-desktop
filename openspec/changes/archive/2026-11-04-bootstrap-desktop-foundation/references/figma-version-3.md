# Figma Version 3.0 foundation reference

Extracted read-only through the configured Figma MCP on 2026-11-04. The supplied node is confirmed as the **Version 3.0** page, `3994:677`, in [GameNode](https://www.figma.com/design/ikcThguwowTdynGz0L2YkS/GameNode?node-id=3994-677).

[figma-version-3-tokens.json](figma-version-3-tokens.json) is the machine-readable snapshot: 40 selected desktop roles, 48 inspected variable definitions, their alias targets and mode IDs, six named text styles, and measured source-node properties. It contains literal resolved values as well as original variable names and IDs. It is planning data to copy into the Slint theme during apply; builds will not contact Figma.

## Selection and scope

Version 3.0 contains several token families and compositions with different neutral colors and dimensions. They are not interchangeable. Use **GameNode / Semantic** (`4945:752`, Dark) and **GameNode / Dimensions** (`4945:758`, Default) as the shared foundation. Their primitive color targets are in **GameNode / Primitives** (`4945:728`, Dark). These variables are actually bound to nodes on Version 3.0; inspected examples include cards `4955:7466` and `4955:7520`.

Use **Configurações (Sam) / Tokens** (`4856:3048`, Dark) for status, raised-surface, and subtle-border roles not supplied by the shared semantic collection. Use the measured Top Bar and primary-button values for chrome and on-primary text. This is an explicit desktop composition of existing Version 3.0 sources, not a claim that every source screen uses one palette.

Keep **Landing V3** aliases/dimensions as alternate source evidence in the JSON. Do not silently replace the chosen background with its `#0B0B0B`, use its pure-white primary text, or merge its differently named spacing scale. The selected `GameNode` spacing scale is `10 / 12 / 16 / 20 / 32`; `Landing V3` uses `8 / 16 / 24 / 40 / 64`. Sizes absent from the shared scale are separately labeled node measurements.

Only visual treatment is shared. Sign-in, the signed-in user, empty library, app menu, and Settings/About remain the entire UI scope of roadmap step 1. Profile editing, premium, notifications, search, social menus, populated game cards, and every other feature shown by the sources are excluded.

## Colors

All values below are sRGB. The JSON retains original normalized RGB(A) floats and alias chains. Eight-digit colors use `#RRGGBBAA`; alpha `1F` is the source's approximately 12.16%, not a newly rounded 15% tint.

| Desktop role | Value | Figma source |
| --- | --- | --- |
| Primary accent | `#F15025` | `color/primary`, `4945:753` → `color/brand/5`, `4945:734` |
| Window background | `#161616` | `color/background`, `4945:754` → `color/paper/0`, `4945:739` |
| Surface | `#262525` | `color/surface`, `4945:755` → `color/paper/1`, `4945:740` |
| Default text | `#D9D9D9` | `color/text-default`, `4945:756` → `color/text`, `4945:748` |
| Dimmed text | `#808080` | `color/text-dimmed`, `4945:757` → `color/dimmed`, `4945:749` |
| Secondary | `#1A1A1A` | `color/secondary`, `4945:750` |
| Tertiary | `#872C13` | `color/tertiary`, `4945:751` |
| Raised surface | `#222222` | `color/bg/raised`, `4856:3052` |
| Subtle border | `#2A2A2A` | `color/border/subtle`, `4856:3053` |
| Danger / subtle danger | `#E5484D` / `#E5484D1F` | `color/danger`, `4856:3055`; `color/danger-subtle`, `4856:3056` |
| Success / subtle success | `#3DD68C` / `#3DD68C1F` | `color/success`, `4856:3057`; `color/success-subtle`, `4856:3058` |
| Warning / subtle warning | `#F5A524` / `#F5A5241F` | `color/warning`, `4856:3059`; `color/warning-subtle`, `4856:3060` |
| Subtle accent | `#F150251F` | `color/accent-subtle`, `4856:3061` |
| Top-bar chrome | `#111111` | Measured fill of Top Bar `4330:1986` |
| Top-bar border | `#202020` | Measured stroke of Top Bar `4330:1986` |
| Primary-button text | `#EDEDED` | Text `I4331:3200;3746:3886` in primary-button instance `4331:3200` |

Do not introduce light-theme or premium color controls. Status colors are available for the foundation's existing success/error/recovery messages. Dimmed text is a secondary role; essential instructions and controls use the default text role.

## Typography

Use **Roboto**, normal width (`wdth = 100`), with weights 400, 500, 600, and 700. All named styles below were resolved through the API and observed in settings source `4933:10443` on the Version 3.0 page. The JSON preserves their exact style IDs.

| Desktop use | Figma style | Size / weight | Line height | Source node |
| --- | --- | --- | --- | --- |
| Small supporting text | `text/xs/regular` | 12 / 400 | Auto | `4933:10449` |
| Body and menu text | `text/sm/regular` | 14 / 400 | Auto | `4933:10470` |
| User identity and labels | `text/sm/semibold` | 14 / 600 | Auto | `4933:10448` |
| Section heading | `text/md/medium` | 16 / 500 | Auto | `4933:10528` |
| Page heading | `text/xl/medium` | 20 / 500 | Auto | `4933:10516` |
| Description or caption | `Landing V3/Caption` | 13 / 400 | 18 px | `4933:10517` |
| Primary-button label | Unnamed node measurement | 14 / 700 | Auto | `I4331:3200;3746:3886` |

These styles have zero letter spacing. `GameNode / Dimensions` also declares the reusable font-size scale `xs=12`, `sm=14`, `md=16`, `lg=18`, `xl=20`. Preserve `Auto` as font-intrinsic line height; do not invent a numeric line height from a measured text bounding box. The 13/18 caption is a separate named style, not a change to that scale.

## Spacing, radii, and measured geometry

| Family | Imported values | Source |
| --- | --- | --- |
| Spacing | `xs=10`, `sm=12`, `md=16`, `lg=20`, `xl=32` | `4945:759`, `763`, `767`, `771`, `775` in GameNode / Dimensions |
| Radii | `xs=2`, `sm=4`, `md=8`, `lg=16`, `xl=32` | `4945:760`, `764`, `768`, `772`, `776` in GameNode / Dimensions |
| Compact padding | 8 px | `4330:3082`, card padding on each edge |
| Control inline padding | 24 px | `4331:3200`, primary-button left/right padding |
| Primary control height | 36 px | `4331:3200`, primary-button height |
| Top-bar height | 54 px | `4330:1986` |
| Top-bar padding | 13 px vertical, 16 px horizontal | `4330:1986` |
| Wordmark box | 72.8 × 28 px | Group `I4330:1986;4111:5253` |

Use 4 px for primary controls and 16 px for panel rounding where applicable. Map Figma pixels to Slint logical pixels and let the OS scale them for display density. Reference frame sizes are not fixed desktop-window requirements.

The Top Bar's `1126` item spacing represents space-between at its 1441 px source width; it is not a reusable gap. The primary button declares 15 px vertical padding despite its fixed 36 px height. Preserve the 36 px control height and 24 px inline padding, and center text vertically in the desktop control instead of reproducing that incompatible combination. A visible keyboard-focus outline using the primary accent is a desktop adaptation; no named focus token was found or invented.

## Brand asset

[assets/gamenode-logo.svg](assets/gamenode-logo.svg) is the exact SVG returned for the Top Bar's stacked GameNode wordmark. It was downloaded from the Figma MCP asset export without redrawing or changing its paths. Preserve its intrinsic dimensions and aspect ratio, and copy it into application assets during apply. The JSON records the source instance and group IDs; no expiring asset URL is needed at build or runtime.

This export establishes brand-asset provenance. Its license is not inferred from the application code's GPL-3.0-only license; the implementation's existing asset/license inventory task still applies. Font binaries and an OS application icon are bundled during implementation, as already scoped in the roadmap.
