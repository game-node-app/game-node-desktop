import { getMatchedGameId, type Row } from '../../../api';

/** The parts of a multi-selection the detail panel summarises. */
export interface SelectionSummary {
  rows: Row[];
  /** Distinct store display names, in selection order. */
  stores: string[];
  installedCount: number;
  allInstalled: boolean;
  /** Sum of the known playtime sources; `null` when no selected row knows its time. */
  totalSeconds: number | null;
  inGameNode: number;
  notOnGameNode: number;
  favorites: number;
  /** Selected rows with a GameNode match; favorites and status changes only apply to these. */
  matched: number;
  /** Every matched selected row is already a favorite. */
  allFavorites: boolean;
}

/**
 * Summarises the selected rows for the multi-selection detail panel. Keys keep the view's
 * order; keys no longer in `rows` (for example a hidden or collapsed row) are ignored.
 */
export function summarizeSelection(selected: string[], rows: Row[]): SelectionSummary {
  const byKey = new Map(rows.map((row) => [row.key, row]));
  const selectedRows: Row[] = [];
  for (const key of selected) {
    const row = byKey.get(key);
    if (row) selectedRows.push(row);
  }

  const stores: string[] = [];
  let installedCount = 0;
  let totalSeconds: number | null = null;
  let inGameNode = 0;
  let favorites = 0;
  let matched = 0;

  for (const row of selectedRows) {
    if (!stores.includes(row.providerName)) stores.push(row.providerName);
    if (row.installed) installedCount += 1;
    if (row.playtime.seconds != null && row.playtime.seconds >= 0) {
      totalSeconds = (totalSeconds ?? 0) + row.playtime.seconds;
    }
    if (row.gamenode != null) inGameNode += 1;
    const matchedGame = getMatchedGameId(row.status);
    if (matchedGame != null) {
      matched += 1;
      if (row.isFavorite) favorites += 1;
    }
  }

  return {
    rows: selectedRows,
    stores,
    installedCount,
    allInstalled: selectedRows.length > 0 && installedCount === selectedRows.length,
    totalSeconds,
    inGameNode,
    notOnGameNode: selectedRows.length - inGameNode,
    favorites,
    matched,
    allFavorites: matched > 0 && favorites === matched,
  };
}
