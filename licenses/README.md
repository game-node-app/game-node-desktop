# License and asset inventory

Application code: GPL-3.0-only, copyright GameNode contributors. The full license
is in `../LICENSE`. Every workspace package inherits this SPDX identifier from
`workspace.package.license`. GameNode Desktop selects Slint's GPLv3 licensing
option; no commercial or royalty-free Slint license is selected.

GameNode artwork: GPL-3.0-only, attribution **GameNode**. The project owner chose
GPLv3 for the wordmark and application icon on 2026-11-04, separately from the
code license. Distribute `LICENSE` and this attribution with copies.

| Asset | Provenance | License / attribution |
| --- | --- | --- |
| `crates/app/ui/icons/gamenode-logo.svg` | Exact Figma Version 3.0 export, group `I4330:1986;4111:5253`, copied from the change reference | GPL-3.0-only / GameNode |
| `crates/app/ui/icons/gamenode.png` | Existing mobile application icon, `game-node-clients/apps/mobile/assets/android/icon/drawable-xxxhdpi-icon.png` | GPL-3.0-only / GameNode |
| `crates/app/ui/fonts/Roboto-Variable.ttf` | Google Fonts `ofl/roboto/Roboto[wdth,wght].ttf`, unmodified variable font including weights 400/500/600/700 | OFL-1.1 / The Roboto Project Authors |

`assets.json` records immutable download URLs and SHA-256 hashes. The complete
font copyright and SIL Open Font License accompany it in `Roboto-OFL-1.1.txt`.
Roboto retains its upstream license independently of the code and artwork.
The wordmark's intrinsic 72.8:28 aspect ratio must be preserved.

This inventory covers assets currently checked into the repository. Dependency
notices and their offline About presentation must additionally be included by
the application/packaging tasks; this inventory is not a dependency-license bill
of materials.
