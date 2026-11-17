import React from 'react';
import { normalizeSettingsSearch, settingsTextMatches } from '../search';

export interface SettingsSearchState {
  /** Raw query typed in the navigation search field. */
  query: string;
  /** NFD-normalized, lowercased query used for every comparison. */
  normalized: string;
}

const SettingsSearchContext = React.createContext<SettingsSearchState>({
  query: '',
  normalized: '',
});

export interface SettingsSearchProviderProps {
  query: string;
  children?: React.ReactNode;
}

/**
 * Publishes the active Settings search to the panel content, so rows built with
 * `SettingsRow` can mark themselves for highlighting and scroll-into-view
 * (design D2). Outside Settings the default empty query disables matching.
 */
export function SettingsSearchProvider({
  query,
  children,
}: SettingsSearchProviderProps) {
  const value = React.useMemo<SettingsSearchState>(
    () => ({ query, normalized: normalizeSettingsSearch(query) }),
    [query],
  );

  return (
    <SettingsSearchContext.Provider value={value}>
      {children}
    </SettingsSearchContext.Provider>
  );
}

export function useSettingsSearch(): SettingsSearchState {
  return React.useContext(SettingsSearchContext);
}

/** True when the given row label contains the active search text. */
export function useSettingsSearchMatch(label: React.ReactNode): boolean {
  const search = useSettingsSearch();
  if (typeof label !== 'string') return false;
  return settingsTextMatches(label, search.normalized);
}
