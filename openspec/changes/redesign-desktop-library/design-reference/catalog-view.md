# Catalog view — "Explorar no GameNode" (no Figma frame)

Built only from the primitives in `components.md`, the tokens in `tokens.md` and the copy in `copy.md` (`Catalog::`).
It reuses the library shell: top bar, sidebar, status bar and the detail-pane slot stay in place.

## Entry points
- Sidebar › Navegar › **Explorar no GameNode** (`IconWorldSearch`). The NavItem is active while the view is shown.
- The "No results" state button **Buscar no GameNode** opens the view with the library search text prefilled.
- Command palette action **Buscar “{q}” no GameNode** opens the view with `{q}` prefilled.
- The app state is `activeDestination = 'catalog'`. Picking any other sidebar item returns to the library.
- Signed-out sessions do not show the NavItem. Opening the view through a deep link while signed out shows the sign-in empty state.

## Layout (main area, 1200 × 790 at the reference size)
```
┌ Header (padding 24/24/16) ───────────────────────────────────────────────┐
│ "Explorar no GameNode"  22/600 text-primary                               │
│ subtitle 13 text-secondary: "Encontre jogos que não estão neste PC…"      │
├ Toolbar (padding 0/24, h 34, gap 8) ─────────────────────────────────────┤
│ [search 480×34, IconSearch, placeholder "Buscar no catálogo do GameNode", │
│  autofocus, clear IconX]                                                   │
├ Results (padding 16/24/24, scrolls) ─────────────────────────────────────┤
│ grid of GameTile (172 wide, gap 16), virtualized like the library grid    │
└───────────────────────────────────────────────────────────────────────────┘
```
- The search is debounced by 300 ms. The query is sent only when its trimmed length is at least 2.
- Results come from the existing engine search (`GameApi::search_page`, one request per query; there is no paging).
  When the answer reports `complete == Some(false)`, show the caption "Os resultados podem estar incompletos. Refine a busca."
  (11px text-dimmed, centered) below the grid. A newer query supersedes an in-flight one: its stale answer is dropped
  (compare the query string echoed in the result).
- Each result is a `GameTile` with:
  - cover from the GameNode cover URL. These are remote images, so the tile uses the same `<img loading="lazy" decoding="async">` with the
    remote URL; there is no `gamenode-asset://` cover yet. If no cover is available, show the gradient placeholder and the title overlay.
  - title, plus a caption showing the release year (or nothing).
  - badges, in this order: **Na sua biblioteca** (Pill neutral, `IconBookmark`) when the account library cache has an entry for the
    game id; **Neste PC** (Pill success, `IconDeviceDesktop`) when any local library row is matched to that game id.
  - no play overlay. Hover shows a ring of `border-strong`, and selection shows the 1.5px brand ring.
- Selecting a tile opens the detail-pane slot with a **catalog detail** variant:
  - hero (196) using the cover, title 15/600, release year as a `KeyValueRow` when known;
  - Primary md full-width **Adicionar ao GameNode…** opens the existing Add to GameNode dialog for that single game id
    (status + collections). It is hidden when the game is already in the library; in that case a
    Secondary **Ver no GameNode** (`IconExternalLink`) opens `/game/<id>` in the browser;
  - when the game is on this PC, a Ghost **Mostrar na biblioteca** button switches to the library with that row selected
    and scrolled into view.
- Context menu on a tile: Adicionar ao GameNode… / Ver no GameNode / Mostrar na biblioteca (when applicable).
  The menu uses the shared menu surface.

## States (icon-circle layout from `07-L5-states.txt`, reused)
| state | icon | title | body | action |
|---|---|---|---|---|
| idle (query < 2) | IconWorldSearch | Buscar no catálogo do GameNode | Digite pelo menos 2 caracteres | — |
| loading first page | — | 12 skeleton tiles (the same skeleton as the scanning panel) | — | — |
| no results | IconMoodEmpty | Nenhum resultado para “{q}” | — | Ghost "Limpar busca" |
| error | IconAlertTriangle (warning) | Não foi possível buscar no GameNode | — | Secondary "Tentar novamente" |
| offline / unreachable | IconCloudOff | Não foi possível acessar o GameNode. A busca precisa de conexão. | — | Secondary "Tentar novamente" |

## Keyboard
- `/` or `Ctrl+F` focuses the search. Arrow keys move the tile focus the same way as the library grid.
- `Enter` opens the detail pane, `Shift+F10` or the Menu key opens the context menu, and `Esc` clears the selection,
  then the search.

## Engine contract
- The input `catalogSearch { query }` emits a new `Effect::CatalogSearch { query }` handled like `SearchGames`
  (`search_page`), answered by `Input::CatalogSearchDone { query, result }`. The view gets
  `catalog: { query, status: idle|loading|ready|error|offline, results: CatalogResult[], incomplete: bool }`, where
  `CatalogResult = { gameId, title, year?, coverUrl?, inLibrary: bool, localKey?: LibraryKey }`.
  `catalogClear` resets it to idle. Covers in results are the server URLs shown directly in `<img>`; they are not
  downloaded into the cover cache.
- `inLibrary` comes from the account library cache. `localKey` is the first local row whose match has `gameId`.
- When an add completes from this view, the account library cache entry for that game is refreshed, which makes
  `inLibrary` flip to true.
- The mock host returns results from a fixture of 40 fictional catalog games, filtered by case-insensitive substring.
