import React, { useEffect, useRef } from 'react';
import { commands, useLibraryView } from '../../api';
import { useUiStore, closeContextMenu, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export const ContextMenu: React.FC = () => {
  const contextMenu = useUiStore((s) => s.contextMenu);
  const library = useLibraryView();
  const { t, locale } = useI18n();
  const menuRef = useRef<HTMLDivElement>(null);

  const isOpen = contextMenu.open && contextMenu.entryKey != null;
  const entryKey = contextMenu.entryKey ?? '';
  const isManual = entryKey.startsWith('manual:');

  // Close on Escape or click outside
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeContextMenu();
      }
    };

    const handlePointerDown = (e: MouseEvent | TouchEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        closeContextMenu();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('mousedown', handlePointerDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('mousedown', handlePointerDown);
    };
  }, [isOpen]);

  if (!isOpen) return null;

  const row = library?.rows.find((r) => r.key === entryKey);
  const actions = library?.actions;

  const canPlay = actions?.play === 'available';
  const showInstall = typeof actions?.install !== 'object' || actions.install.disabled !== 'noInstallRule';
  const canInstall = actions?.install === 'available';
  const canMatch = actions?.matchGame === 'available';
  const isChangeMatch = actions?.changeMatch ?? false;
  const canAdd = actions?.addToGamenode === 'available';
  const isRunning = row?.running ?? false;
  const canTools = !isRunning;

  const handlePlay = () => {
    if (!canPlay) return;
    commands.launchSelected('play');
    closeContextMenu();
  };

  const handleInstall = () => {
    if (!canInstall) return;
    commands.launchSelected('install');
    closeContextMenu();
  };

  const handleMatch = () => {
    if (!canMatch) return;
    commands.libraryInput('openMatch');
    closeContextMenu();
  };

  const handleAdd = () => {
    if (!canAdd) return;
    commands.libraryInput('openAdd');
    closeContextMenu();
  };

  const handleTools = () => {
    if (!canTools) return;
    commands.openTools(entryKey);
    closeContextMenu();
  };

  const handleEditGame = () => {
    uiStore.setState((prev) => ({ ...prev, localEditorEntryKey: entryKey }));
    closeContextMenu();
  };

  const handleSaveProtection = () => {
    uiStore.setState((prev) => ({ ...prev, saveProtectionEntryKey: entryKey }));
    commands.loadLocalHistory(entryKey.replace('manual:', ''));
    closeContextMenu();
  };

  const handleRemove = () => {
    uiStore.setState((prev) => ({ ...prev, removeModalEntryKey: entryKey }));
    closeContextMenu();
  };

  const matchLabel = isChangeMatch
    ? t('Library::Change match…')
    : locale === 'pt-BR'
      ? t('Library::Match…')
      : 'Match game';

  const editLabel = locale === 'pt-BR' ? t('DRM-free::Game settings') : 'Edit game';
  const saveLabel = locale === 'pt-BR' ? t('DRM-free::Local save protection') : 'Save protection';
  const removeLabel = locale === 'pt-BR' ? t('DRM-free::Remove from library') : 'Remove';

  // Position adjustments to prevent overflow
  const posX = Math.max(8, Math.min(contextMenu.x, (typeof window !== 'undefined' ? window.innerWidth : 800) - 220));
  const posY = Math.max(8, Math.min(contextMenu.y, (typeof window !== 'undefined' ? window.innerHeight : 600) - 300));

  const itemStyle = (enabled: boolean, danger = false): React.CSSProperties => ({
    padding: '8px 14px',
    fontSize: 'var(--font-size-sm, 14px)',
    color: danger
      ? 'var(--color-danger, #E5484D)'
      : enabled
        ? 'var(--color-text-default, #D9D9D9)'
        : 'var(--color-text-dimmed, #808080)',
    opacity: enabled ? 1 : 0.5,
    cursor: enabled ? 'pointer' : 'default',
    backgroundColor: 'transparent',
    border: 'none',
    textAlign: 'left',
    width: '100%',
    display: 'flex',
    alignItems: 'center',
    gap: '8px',
    borderRadius: 'var(--radius-xs, 2px)',
    userSelect: 'none',
  });

  return (
    <div
      ref={menuRef}
      role="menu"
      data-testid="library-context-menu"
      style={{
        position: 'fixed',
        left: `${posX}px`,
        top: `${posY}px`,
        zIndex: 1000,
        minWidth: '190px',
        backgroundColor: 'var(--color-surface, #262525)',
        border: '1px solid var(--color-border-subtle, #2A2A2A)',
        borderRadius: 'var(--radius-md, 8px)',
        boxShadow: 'var(--shadow-alert, 0 24px 64px rgba(0, 0, 0, 0.6))',
        padding: '6px',
        display: 'flex',
        flexDirection: 'column',
      }}
    >
      {/* Play */}
      <button
        type="button"
        role="menuitem"
        data-testid="context-menu-play"
        disabled={!canPlay}
        onClick={handlePlay}
        style={itemStyle(canPlay)}
      >
        {t('Library::Play')}
      </button>

      {/* Install */}
      {showInstall && (
        <button
          type="button"
          role="menuitem"
          data-testid="context-menu-install"
          disabled={!canInstall}
          onClick={handleInstall}
          style={itemStyle(canInstall)}
        >
          {t('Library::Install')}
        </button>
      )}

      {/* Match game / Change match */}
      <button
        type="button"
        role="menuitem"
        data-testid="context-menu-match"
        aria-label={isChangeMatch ? 'Change match' : 'Match game'}
        disabled={!canMatch}
        onClick={handleMatch}
        style={itemStyle(canMatch)}
      >
        {matchLabel}
      </button>

      {/* Add to GameNode */}
      <button
        type="button"
        role="menuitem"
        data-testid="context-menu-add"
        aria-label="Add to GameNode"
        disabled={!canAdd}
        onClick={handleAdd}
        style={itemStyle(canAdd)}
      >
        {t('Library::Add to GameNode…')}
      </button>

      {/* Game tools */}
      <button
        type="button"
        role="menuitem"
        data-testid="context-menu-tools"
        aria-label="Game tools"
        disabled={!canTools}
        onClick={handleTools}
        style={itemStyle(canTools)}
      >
        {t('Library::Game tools…')}
      </button>

      {/* Manual Game Actions */}
      {isManual && (
        <>
          <div
            style={{
              height: '1px',
              backgroundColor: 'var(--color-border-subtle, #2A2A2A)',
              margin: '4px 0',
            }}
          />

          <button
            type="button"
            role="menuitem"
            data-testid="context-menu-edit"
            aria-label="Edit game"
            onClick={handleEditGame}
            style={itemStyle(true)}
          >
            {editLabel}
          </button>

          <button
            type="button"
            role="menuitem"
            data-testid="context-menu-save-protection"
            aria-label="Save protection"
            onClick={handleSaveProtection}
            style={itemStyle(true)}
          >
            {saveLabel}
          </button>

          <button
            type="button"
            role="menuitem"
            data-testid="context-menu-remove"
            aria-label="Remove"
            onClick={handleRemove}
            style={itemStyle(true, true)}
          >
            {removeLabel}
          </button>
        </>
      )}
    </div>
  );
};
