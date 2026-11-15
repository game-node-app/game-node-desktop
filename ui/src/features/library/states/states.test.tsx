import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nProvider } from '../../../i18n';
import { FirstRunGuide } from './FirstRunGuide';
import { NoGamesState } from './NoGamesState';
import { NoInstalledState } from './NoInstalledState';
import { NoResultsState } from './NoResultsState';
import { ScanningPanel } from './ScanningPanel';
import type { ScanProgress } from '../../../api';

function wrap(ui: ReactElement, locale: 'en' | 'pt-BR' = 'en') {
  return render(<I18nProvider initialLocale={locale}>{ui}</I18nProvider>);
}

function progress(overrides: Partial<ScanProgress> = {}): ScanProgress {
  return {
    provider: 'steam',
    name: 'Steam',
    state: 'scanning',
    found: null,
    ...overrides,
  };
}

describe('library states', () => {
  it('renders the three first-run steps with done states and the first pending action primary', () => {
    const onConnect = vi.fn();
    const onChooseFolder = vi.fn();
    const onRescan = vi.fn();
    wrap(
      <FirstRunGuide
        signedIn
        username="cassi"
        storeConnected={false}
        folderAdded={false}
        onConnect={onConnect}
        onChooseFolder={onChooseFolder}
        onRescan={onRescan}
      />,
    );

    expect(screen.getByRole('heading', { name: 'Your library is empty' })).toBeInTheDocument();
    expect(screen.getByTestId('first-run-step-1')).toHaveAttribute('data-done', 'true');
    expect(screen.getByTestId('first-run-step-1')).toHaveTextContent('Account cassi connected');
    expect(screen.getByTestId('first-run-step-2')).toHaveAttribute('data-done', 'false');
    expect(screen.getByTestId('first-run-step-3')).toHaveAttribute('data-done', 'false');

    const connect = screen.getByTestId('first-run-action-2');
    expect(connect.className).toContain('bg-brand');
    fireEvent.click(connect);
    expect(onConnect).toHaveBeenCalledTimes(1);

    const choose = screen.getByTestId('first-run-action-3');
    expect(choose.className).toContain('border-border-subtle');
    fireEvent.click(choose);
    expect(onChooseFolder).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByTestId('first-run-rescan'));
    expect(onRescan).toHaveBeenCalledTimes(1);
  });

  it('hides the actions of completed first-run steps', () => {
    wrap(
      <FirstRunGuide
        signedIn
        username="cassi"
        storeConnected
        folderAdded
        onConnect={() => undefined}
        onChooseFolder={() => undefined}
        onRescan={() => undefined}
      />,
    );

    expect(screen.getByTestId('first-run-step-2')).toHaveAttribute('data-done', 'true');
    expect(screen.getByTestId('first-run-step-3')).toHaveAttribute('data-done', 'true');
    expect(screen.queryByTestId('first-run-action-2')).not.toBeInTheDocument();
    expect(screen.queryByTestId('first-run-action-3')).not.toBeInTheDocument();
  });

  it('names the stores an empty scan looked at and offers Rescan', () => {
    const onRescan = vi.fn();
    wrap(<NoGamesState stores={['Steam', 'GOG']} onRescan={onRescan} />);

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent('No games found.');
    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'Looked in: Steam, GOG',
    );
    fireEvent.click(screen.getByTestId('library-empty-rescan-btn'));
    expect(onRescan).toHaveBeenCalledTimes(1);
  });

  it('explains the installed filter and offers to show all games', () => {
    const onShowAll = vi.fn();
    wrap(<NoInstalledState onShowAll={onShowAll} />);

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'Nothing is installed on this PC.',
    );
    fireEvent.click(screen.getByTestId('library-empty-show-all-btn'));
    expect(onShowAll).toHaveBeenCalledTimes(1);
  });

  it('distinguishes a search term from filters and counts active filters', () => {
    const onClear = vi.fn();
    const onSearch = vi.fn();
    wrap(
      <NoResultsState
        search="lumen drift"
        activeFilterCount={2}
        onClearFilters={onClear}
        onSearchGameNode={onSearch}
      />,
    );

    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      'No game matches “lumen drift”',
    );
    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      '2 active filters may be hiding results.',
    );
    fireEvent.click(screen.getByTestId('library-empty-clear-filters-btn'));
    expect(onClear).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId('library-empty-search-gamenode-btn'));
    expect(onSearch).toHaveBeenCalledTimes(1);
  });

  it('uses the singular filter sentence and the filter-only title without a search term', () => {
    wrap(
      <NoResultsState
        search=""
        activeFilterCount={1}
        onClearFilters={() => undefined}
        onSearchGameNode={() => undefined}
      />,
    );

    expect(screen.getByRole('heading', { name: 'No game matches the filters' })).toBeInTheDocument();
    expect(screen.getByTestId('library-empty-state')).toHaveTextContent(
      '1 active filter may be hiding results.',
    );
    expect(screen.queryByTestId('library-empty-search-gamenode-btn')).not.toBeInTheDocument();
  });

  it('shows each provider state, found count and the DRM-free pseudo row', () => {
    wrap(
      <ScanningPanel
        progress={[
          progress({ provider: 'steam', name: 'Steam', state: 'done', found: 164 }),
          progress({ provider: 'epic', name: 'Epic Games Store', state: 'scanning' }),
          progress({ provider: 'drm-free', name: 'DRM-free folders', state: 'waiting' }),
        ]}
      />,
    );

    expect(screen.getByTestId('scan-progress-steam')).toHaveAttribute('data-state', 'done');
    expect(screen.getByTestId('scan-progress-steam')).toHaveTextContent('164');
    expect(screen.getByTestId('scan-progress-epic')).toHaveAttribute('data-state', 'scanning');
    expect(screen.getByTestId('scan-progress-drm-free')).toHaveAttribute(
      'data-state',
      'waiting',
    );
    expect(screen.getByTestId('scan-progress-drm-free')).toHaveTextContent(
      'DRM-free folders',
    );
    expect(screen.queryByTestId('library-scanning-skeletons')).not.toBeInTheDocument();
  });

  it('renders twelve skeletons while the first scan has no rows', () => {
    wrap(<ScanningPanel progress={[progress()]} showSkeletons />);
    expect(screen.getByTestId('library-scanning-skeletons').children).toHaveLength(12);
  });

  it('localizes the scanning panel in Brazilian Portuguese', () => {
    wrap(<ScanningPanel progress={[progress({ provider: 'drm-free' })]} />, 'pt-BR');
    expect(screen.getByTestId('library-scanning-panel')).toHaveTextContent(
      'Verificando a biblioteca…',
    );
    expect(screen.getByTestId('scan-progress-drm-free')).toHaveTextContent('Pastas DRM-free');
  });
});
