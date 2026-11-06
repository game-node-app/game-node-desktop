import React, { useContext, useEffect } from 'react';
import { MantineContext, MantineProvider, Menu } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import {
  IconDownload,
  IconLink,
  IconPencil,
  IconPlayerPlay,
  IconPlus,
  IconShieldCheck,
  IconTool,
  IconTrash,
} from '@tabler/icons-react';
import { commands, useLibraryView } from '../../api';
import { useUiStore, closeContextMenu, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

const itemBase =
  'flex w-full items-center gap-2 rounded-xs px-3.5 py-compact text-left text-sm transition-colors data-[disabled]:cursor-default data-[disabled]:text-text-dimmed data-[disabled]:opacity-50 data-[disabled]:hover:bg-transparent';
const itemClass = `${itemBase} text-text-default hover:bg-surface`;
const dangerItem = `${itemBase} text-danger hover:bg-danger-subtle`;

export const ContextMenu: React.FC = () => {
  const contextMenu = useUiStore((s) => s.contextMenu);
  const library = useLibraryView();
  const { t, locale } = useI18n();
  const mantineContext = useContext(MantineContext);

  const isOpen = contextMenu.open && contextMenu.entryKey != null;
  const entryKey = contextMenu.entryKey ?? '';
  const isManual = entryKey.startsWith('manual:');

  // Close on Escape. Menu handles outside clicks and item dismissal.
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeContextMenu();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
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
  const canRemove = row != null;

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

  const menu = (
    <Menu
      opened
      onClose={closeContextMenu}
      position="bottom-start"
      offset={0}
      floatingStrategy="fixed"
      zIndex={1000}
      trapFocus={false}
      returnFocus={false}
      closeOnEscape={false}
      transitionProps={{ duration: 0 }}
    >
      {/* Zero-size anchor at the right-click / keyboard-open coordinates. */}
      <Menu.Target>
        <div
          aria-hidden
          style={{
            position: 'fixed',
            left: `${posX}px`,
            top: `${posY}px`,
            width: 0,
            height: 0,
          }}
        />
      </Menu.Target>

      <Menu.Dropdown
        data-testid="library-context-menu"
        aria-label="Game actions"
        className="min-w-[190px] rounded-md border border-border-subtle bg-surface p-1.5 shadow-alert"
      >
        {/* Play */}
        <Menu.Item
          data-testid="context-menu-play"
          disabled={!canPlay}
          onClick={handlePlay}
          className={itemClass}
        >
          <IconPlayerPlay size={14} stroke={1.75} aria-hidden />
          {t('Library::Play')}
        </Menu.Item>

        {/* Install */}
        {showInstall && (
          <Menu.Item
            data-testid="context-menu-install"
            disabled={!canInstall}
            onClick={handleInstall}
            className={itemClass}
          >
            <IconDownload size={14} stroke={1.75} aria-hidden />
            {t('Library::Install')}
          </Menu.Item>
        )}

        {/* Match game / Change match */}
        <Menu.Item
          data-testid="context-menu-match"
          aria-label={isChangeMatch ? 'Change match' : 'Match game'}
          disabled={!canMatch}
          onClick={handleMatch}
          className={itemClass}
        >
          <IconLink size={14} stroke={1.75} aria-hidden />
          {matchLabel}
        </Menu.Item>

        {/* Add to GameNode */}
        <Menu.Item
          data-testid="context-menu-add"
          aria-label="Add to GameNode"
          disabled={!canAdd}
          onClick={handleAdd}
          className={itemClass}
        >
          <IconPlus size={14} stroke={1.75} aria-hidden />
          {t('Library::Add to GameNode…')}
        </Menu.Item>

        {/* Game tools */}
        <Menu.Item
          data-testid="context-menu-tools"
          aria-label="Game tools"
          disabled={!canTools}
          onClick={handleTools}
          className={itemClass}
        >
          <IconTool size={14} stroke={1.75} aria-hidden />
          {t('Library::Game tools…')}
        </Menu.Item>

        {(isManual || canRemove) && (
          <Menu.Divider className="my-1 border-border-subtle" />
        )}

        {/* Manual Game Actions */}
        {isManual && (
          <>
            <Menu.Item
              data-testid="context-menu-edit"
              aria-label="Edit game"
              onClick={handleEditGame}
              className={itemClass}
            >
              <IconPencil size={14} stroke={1.75} aria-hidden />
              {editLabel}
            </Menu.Item>

            <Menu.Item
              data-testid="context-menu-save-protection"
              aria-label="Save protection"
              onClick={handleSaveProtection}
              className={itemClass}
            >
              <IconShieldCheck size={14} stroke={1.75} aria-hidden />
              {saveLabel}
            </Menu.Item>
          </>
        )}

        {/* Remove from library — available for manual and store entries */}
        {canRemove && (
          <Menu.Item
            data-testid="context-menu-remove"
            aria-label="Remove"
            onClick={handleRemove}
            className={dangerItem}
          >
            <IconTrash size={14} stroke={1.75} aria-hidden />
            {removeLabel}
          </Menu.Item>
        )}
      </Menu.Dropdown>
    </Menu>
  );

  // Mantine v9 primitives require a provider context. The app mounts one in
  // AppProviders; keep standalone renders (component tests) working as well.
  if (mantineContext) {
    return menu;
  }

  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      {menu}
    </MantineProvider>
  );
};
