import React from 'react';
import { commands, useLibraryView } from '../../api';
import { openContextMenu } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { LibraryToolbar } from './LibraryToolbar';
import { GameGrid } from './GameGrid';
import { GameList } from './GameList';
import { DetailPane } from './DetailPane';
import { ContextMenu } from './ContextMenu';
import { formatI18n } from './formatters';

export const LibraryScreen: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const viewMode = library?.layout?.view ?? 'grid';
  const detailOpen = library?.layout?.detailOpen ?? false;
  const empty = library?.empty;

  const handleKeyDown = (e: React.KeyboardEvent) => {
    const targetTag = (e.target as HTMLElement)?.tagName?.toLowerCase();
    if (targetTag === 'input' || targetTag === 'textarea' || targetTag === 'select') {
      if (e.key === 'Escape') {
        (e.target as HTMLElement).blur();
      }
      return;
    }

    if (e.key === 'ArrowUp') {
      commands.libraryInput({ move: { direction: 'up', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'ArrowDown') {
      commands.libraryInput({ move: { direction: 'down', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'ArrowLeft') {
      commands.libraryInput({ move: { direction: 'left', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'ArrowRight') {
      commands.libraryInput({ move: { direction: 'right', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'Home') {
      commands.libraryInput({ move: { direction: 'home', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'End') {
      commands.libraryInput({ move: { direction: 'end', extend: e.shiftKey } });
      e.preventDefault();
    } else if (e.key === 'Enter') {
      if (e.ctrlKey || e.metaKey) {
        commands.libraryInput('openDetails');
      } else {
        commands.activateEntry();
      }
      e.preventDefault();
    } else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
      if (library?.focused) {
        const x = typeof window !== 'undefined' ? window.innerWidth / 2 : 400;
        const y = typeof window !== 'undefined' ? window.innerHeight / 2 : 300;
        openContextMenu(x, y, library.focused);
      }
      e.preventDefault();
    } else if (e.key === 'F5') {
      commands.rescanLibrary();
      e.preventDefault();
    }
  };

  return (
    <div
      data-testid="library-screen"
      tabIndex={0}
      onKeyDown={handleKeyDown}
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        outline: 'none',
        overflow: 'hidden',
        position: 'relative',
        backgroundColor: 'var(--color-background, #161616)',
      }}
    >
      <LibraryToolbar />

      <div
        style={{
          display: 'flex',
          flex: 1,
          width: '100%',
          height: 'calc(100% - 54px)',
          overflow: 'hidden',
          position: 'relative',
        }}
      >
        {/* Empty States */}
        {empty && (
          <div
            data-testid="library-empty-state"
            style={{
              flex: 1,
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              padding: '32px',
              textAlign: 'center',
              gap: '12px',
            }}
          >
            {typeof empty === 'object' && 'noGames' in empty && (
              <>
                <h2
                  style={{
                    fontSize: 'var(--font-size-xl, 20px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                  }}
                >
                  {t('Library::No installed games found.')}
                </h2>
                <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                  {formatI18n(
                    t('Library::Looked in: {}'),
                    empty.noGames.stores.join(', '),
                  )}
                </div>
                <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                  {t('Library::Install a game, then choose Rescan.')}
                </div>
              </>
            )}

            {empty === 'nothingMatches' && (
              <>
                <h2
                  style={{
                    fontSize: 'var(--font-size-xl, 20px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                  }}
                >
                  {t('Library::No games match.')}
                </h2>
                <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                  {t('Library::Nothing in this view matches your search.')}
                </div>
                <button
                  type="button"
                  data-testid="library-empty-clear-search-btn"
                  onClick={() => commands.libraryInput({ setSearch: '' })}
                  style={{
                    padding: '8px 16px',
                    backgroundColor: 'transparent',
                    border: '1px solid var(--color-border-subtle, #2A2A2A)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                    cursor: 'pointer',
                  }}
                >
                  {t('Library::Clear search')}
                </button>
              </>
            )}
          </div>
        )}

        {/* View Grid or List */}
        {!empty && (
          <div
            style={{
              flex: 1,
              display: 'flex',
              height: '100%',
              overflow: 'hidden',
            }}
          >
            {viewMode === 'grid' ? <GameGrid /> : <GameList />}
          </div>
        )}

        {/* Detail Pane */}
        {detailOpen && <DetailPane />}
      </div>

      {/* Context Menu */}
      <ContextMenu />
    </div>
  );
};
