import React, { useEffect, useId, useMemo, useRef, useState } from 'react';
import { FocusTrap, OptionalPortal, Overlay } from '@mantine/core';
import {
  IconCompass,
  IconDeviceGamepad2,
  IconFolder,
  IconKeyboard,
  IconLayoutGrid,
  IconLayoutList,
  IconRefresh,
  IconSearch,
  IconSettings,
} from '@tabler/icons-react';
import type { TablerIcon } from '@tabler/icons-react';
import { commands } from '../../api/commands';
import { useLibraryView } from '../../api/useSnapshot';
import {
  createDefaultFilters,
  filtersAreDefault,
  type LibraryView,
  type PaletteEntry,
} from '../../api/types';
import {
  Kbd,
  KbdCombo,
  SectionLabel,
} from '../../components/ui';
import { cx } from '../../components/ui/cx';
import { useFocusRestore } from '../../hooks/useFocusRestore';
import { useModalLayer } from '../../components/modalLayer';
import {
  closePalette,
  openContextMenu,
  openSettingsModal,
  openShortcuts,
  setActiveDestination,
  useUiStore,
  type SettingsTopic,
} from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from '../library/formatters';
import { contextMenuPointFor, contextMenuRectFor } from '../library/contextMenuAnchor';

type GameAction = 'play' | 'install' | 'open';

interface GameItem {
  kind: 'game';
  id: string;
  entry: PaletteEntry;
  action: GameAction;
}

interface ActionItem {
  kind: 'action';
  id: string;
  label: string;
  icon: TablerIcon;
  run: () => void | Promise<void>;
}

interface SettingsItem {
  kind: 'settings';
  id: string;
  label: string;
  topic: SettingsTopic;
}

type PaletteItem = GameItem | ActionItem | SettingsItem;

interface SettingsEntry {
  topic: SettingsTopic;
  topicKey: string;
  itemKey: string;
}

/**
 * The static Settings section of the palette. Each row names the existing settings topic
 * and section labels and opens that topic through `openSettingsModal`, which never drops
 * an unsaved Settings draft.
 */
const SETTINGS_ENTRIES: SettingsEntry[] = [
  { topic: 'account', topicKey: 'settings.account', itemKey: 'settings.sessionAndIdentity' },
  { topic: 'account', topicKey: 'settings.account', itemKey: 'settings.sync' },
  { topic: 'account', topicKey: 'settings.account', itemKey: 'settings.playtimeSource' },
  { topic: 'stores', topicKey: 'settings.stores', itemKey: 'settings.detectedStores' },
  { topic: 'stores', topicKey: 'settings.stores', itemKey: 'drmFree.drmFreeGames' },
  // The desktop-shell palette scenario names the shared-access member explicitly. "Family"
  // reaches the Stores topic, whose account cards own the family-shared counts.
  { topic: 'stores', topicKey: 'settings.stores', itemKey: 'settings.familyMember' },
  { topic: 'plugins', topicKey: 'plugins.navTitle', itemKey: 'plugins.navDescription' },
  { topic: 'backups', topicKey: 'settings.backups', itemKey: 'settings.automaticProtection' },
  { topic: 'backups', topicKey: 'settings.backups', itemKey: 'Backup destination' },
  { topic: 'tools', topicKey: 'settings.tools', itemKey: 'Tool download cache' },
  { topic: 'tools', topicKey: 'settings.tools', itemKey: 'Supported tools' },
  { topic: 'about', topicKey: 'settings.about', itemKey: 'settings.aboutGamenodeDesktop' },
];

const GAME_ACTION_LABEL: Record<GameAction, string> = {
  play: 'Palette::Play',
  install: 'Palette::Install',
  open: 'Palette::Open',
};

function gameActionFor(library: LibraryView | null | undefined, entry: PaletteEntry): GameAction {
  if (entry.installed) return 'play';
  const row = library?.rows.find((candidate) => candidate.key === entry.key);
  return row?.owned ? 'install' : 'open';
}

function gameSubtitle(t: (key: string) => string, entry: PaletteEntry): string {
  if (entry.installed) {
    return entry.playtimeLabel
      ? formatI18n(t('Palette::{} · Installed · {}'), entry.providerName, entry.playtimeLabel)
      : `${entry.providerName} · ${t('Library::Installed')}`;
  }
  return formatI18n(t('Palette::{} · Not installed'), entry.providerName);
}

/**
 * The command palette (design D11). It uses the shared modal layer for stacking and
 * Escape, Mantine's focus trap for Tab handling and the shared focus restore, but not
 * `AppModal`: the palette is a 600 px surface anchored 96 px from the top.
 *
 * Games come from `library.paletteIndex`, which the engine publishes for every non-hidden
 * row independently of the active filters. Enter selects the row (revealing it first when
 * a filter hides it) and then runs the engine's own launch command. Shift+F10 opens the
 * regular library context menu for the active game.
 */
export const CommandPalette: React.FC = () => {
  const opened = useUiStore((state) => state.paletteOpen);
  const library = useLibraryView();
  const { t } = useI18n();
  const layer = useModalLayer({ opened, closeOnEscape: true, onClose: closePalette });
  useFocusRestore(opened);

  const baseId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const optionRefs = useRef(new Map<number, HTMLButtonElement>());
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);

  useEffect(() => {
    if (!opened) return;
    setQuery('');
    setActiveIndex(0);
    // The focus trap lands on the input; focusing explicitly keeps focus there after a
    // reopen even when the trap was already active.
    inputRef.current?.focus();
  }, [opened]);

  const groups = useMemo(() => {
    const text = query.trim();
    const needle = text.toLowerCase();
    const paletteIndex = library?.paletteIndex ?? [];

    const matchingGames = text
      ? paletteIndex.filter((entry) => entry.title.toLowerCase().includes(needle))
      : paletteIndex;

    const games: GameItem[] = matchingGames.slice(0, 5).map((entry) => ({
      kind: 'game',
      id: `game:${entry.key}`,
      entry,
      action: gameActionFor(library, entry),
    }));

    const actions: ActionItem[] = ([
      {
        kind: 'action',
        id: 'action:add-drm-free',
        label: t('Palette::Add DRM-free game…'),
        icon: IconFolder,
        run: () => commands.pickLocalExecutable(null),
      },
      {
        kind: 'action',
        id: 'action:rescan',
        label: t('Palette::Rescan library'),
        icon: IconRefresh,
        run: () => commands.rescanLibrary(),
      },
      {
        kind: 'action',
        id: 'action:shortcuts',
        label: t('Palette::Open keyboard shortcuts'),
        icon: IconKeyboard,
        run: () => openShortcuts(),
      },
      {
        kind: 'action',
        id: 'action:toggle-view',
        label: t('Palette::Toggle grid / list'),
        icon: library?.layout.view === 'list' ? IconLayoutGrid : IconLayoutList,
        run: () => {
          if (!library) return;
          setActiveDestination('library');
          void commands.libraryInput({
            setView: library.layout.view === 'grid' ? 'list' : 'grid',
          });
        },
      },
    ] as ActionItem[]).filter((action) => !needle || action.label.toLowerCase().includes(needle));

    if (text && matchingGames.length === 0) {
      actions.unshift({
        kind: 'action',
        id: 'action:search-gamenode',
        label: formatI18n(t('Palette::Search “{}” on GameNode'), text),
        icon: IconCompass,
        run: () => {
          setActiveDestination('catalog');
          void commands.libraryInput({ catalogSearch: text });
        },
      });
    }

    const settings: SettingsItem[] = (SETTINGS_ENTRIES.map((entry) => {
      const topicLabel = t(entry.topicKey);
      const itemLabel = t(entry.itemKey);
      return {
        kind: 'settings',
        id: `settings:${entry.topic}:${entry.itemKey}`,
        label: `${t('Palette::Settings')} › ${topicLabel} › ${itemLabel}`,
        topic: entry.topic,
      };
    }) as SettingsItem[]).filter((item) => !needle || item.label.toLowerCase().includes(needle));

    const allGroups: Array<{ id: string; label: string; items: PaletteItem[] }> = [
      { id: 'games', label: t('Palette::Games'), items: games.slice(0, 5) },
      { id: 'actions', label: t('Palette::Actions'), items: actions.slice(0, 5) },
      { id: 'settings', label: t('Palette::Settings'), items: settings.slice(0, 5) },
    ];
    return allGroups.filter((group) => group.items.length > 0);
  }, [library, query, t]);

  const flat = useMemo<PaletteItem[]>(
    () => groups.flatMap((group) => group.items),
    [groups],
  );
  const indexById = useMemo(
    () => new Map(flat.map((item, index) => [item.id, index])),
    [flat],
  );

  useEffect(() => {
    setActiveIndex((current) =>
      flat.length === 0 ? 0 : Math.min(current, flat.length - 1),
    );
  }, [flat.length]);

  const activeItem = flat[activeIndex];

  useEffect(() => {
    optionRefs.current.get(activeIndex)?.scrollIntoView({ block: 'nearest' });
  }, [activeIndex]);

  if (!opened) return null;

  /**
   * The engine ignores selection for rows outside the current view. Clear only what hides
   * the requested key (search, filters, scope and collapsed groups), then select it. Every
   * command is awaited so the launch happens after the engine has applied the reveal.
   */
  async function revealKey(key: string): Promise<void> {
    const current = library;
    const visible = current?.rows.some((row) => row.key === key) ?? false;
    if (!visible && current) {
      if (current.search) {
        await commands.libraryInput({ setSearch: '' });
      }
      if (!filtersAreDefault(current.layout.filters)) {
        await commands.libraryInput({ setFilters: createDefaultFilters() });
      }
      if (current.layout.scope !== 'all') {
        await commands.libraryInput({ setScope: 'all' });
      }
      const collapsed = Object.values(current.layout.collapsedGroups).some(
        (ids) => ids != null && ids.length > 0,
      );
      if (collapsed) {
        await commands.libraryInput({ setAllGroupsCollapsed: false });
      }
    }
    await commands.libraryInput({ select: { key, mode: 'replace' } });
  }

  async function runGame(item: GameItem): Promise<void> {
    closePalette();
    if (item.action === 'open') {
      setActiveDestination('library');
    }
    await revealKey(item.entry.key);
    if (item.action === 'open') {
      await commands.libraryInput('openDetails');
    } else {
      await commands.launchSelected(item.action);
    }
  }

  async function openGameMenu(item: GameItem): Promise<void> {
    // Read the real option rectangle while the palette DOM still exists, so the menu lands on
    // the row the user chose instead of the viewport origin. A missing or collapsed rectangle
    // falls back to the viewport centre.
    const index = indexById.get(item.id) ?? 0;
    const rect = contextMenuRectFor(optionRefs.current.get(index));
    const point = contextMenuPointFor(
      rect != null && rect.width > 0 && rect.height > 0 ? rect : null,
    );

    closePalette();
    // The library context menu only renders inside the library screen. From the catalog or the
    // PC ports view the palette must activate the library first; otherwise the menu would be
    // open in the store with no surface, and that invisible open would swallow Ctrl+K.
    setActiveDestination('library');
    try {
      await revealKey(item.entry.key);
    } catch {
      // The engine rejected the reveal. Do not open a menu for a row that was not selected.
      return;
    }
    openContextMenu(point.x, point.y, item.entry.key);
  }

  async function runItem(item: PaletteItem): Promise<void> {
    if (item.kind === 'game') {
      await runGame(item);
      return;
    }
    closePalette();
    if (item.kind === 'settings') {
      openSettingsModal(item.topic);
      return;
    }
    await item.run();
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDivElement>): void {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      if (flat.length === 0) return;
      event.preventDefault();
      setActiveIndex((current) =>
        event.key === 'ArrowDown'
          ? (current + 1) % flat.length
          : (current - 1 + flat.length) % flat.length,
      );
      return;
    }

    if (event.key === 'Enter') {
      // A focused option button already activates through its native click.
      if (event.target instanceof HTMLButtonElement) return;
      if (!activeItem) return;
      event.preventDefault();
      void runItem(activeItem);
      return;
    }

    if (event.key === 'F10' && event.shiftKey) {
      if (activeItem?.kind !== 'game') return;
      event.preventDefault();
      void openGameMenu(activeItem);
    }
  }

  const activeOptionId =
    flat.length > 0 ? `${baseId}-option-${activeIndex}` : undefined;

  return (
    <OptionalPortal>
      <div
        data-testid="command-palette-layer"
        className="fixed inset-0"
        style={{ zIndex: layer.zIndex }}
      >
        <Overlay backgroundOpacity={0.62} onClick={closePalette} />
        <FocusTrap active innerRef={rootRef}>
          <div
            ref={rootRef}
            role="dialog"
            aria-modal="true"
            aria-label={t('Palette::Search games, actions and settings')}
            data-testid="command-palette"
            tabIndex={-1}
            onKeyDown={handleKeyDown}
            className="absolute left-1/2 top-[96px] flex max-h-[min(520px,calc(100vh_-_140px))] w-[600px] max-w-[calc(100vw_-_32px)] -translate-x-1/2 flex-col overflow-hidden rounded-palette border border-border-overlay bg-secondary shadow-toast focus:outline-none"
          >
            <div className="flex h-[52px] shrink-0 items-center gap-[10px] border-b border-border-subtle px-[16px]">
              <IconSearch
                size={16}
                stroke={2}
                aria-hidden="true"
                className="shrink-0 text-text-dimmed"
              />
              <input
                ref={inputRef}
                type="text"
                role="combobox"
                aria-expanded="true"
                aria-controls={`${baseId}-list`}
                aria-autocomplete="list"
                aria-activedescendant={activeOptionId}
                aria-label={t('Palette::Search games, actions and settings')}
                data-testid="command-palette-input"
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setActiveIndex(0);
                }}
                placeholder={t('Palette::Search games, actions and settings')}
                className="min-w-0 flex-1 bg-transparent text-[14px] text-text-primary outline-none placeholder:text-text-dimmed"
              />
              <Kbd>Esc</Kbd>
            </div>

            <div
              id={`${baseId}-list`}
              role="listbox"
              aria-label={t('Palette::Search games, actions and settings')}
              className="min-h-0 flex-1 overflow-y-auto p-[6px]"
            >
              {groups.map((group) => (
                <div
                  key={group.id}
                  data-testid={`palette-group-${group.id}`}
                  role="group"
                  aria-labelledby={`${baseId}-group-${group.id}`}
                >
                  <SectionLabel
                    id={`${baseId}-group-${group.id}`}
                    className="px-[10px] pb-[4px] pt-[8px]"
                  >
                    {group.label}
                  </SectionLabel>
                  {group.items.map((item) => {
                    const index = indexById.get(item.id) ?? 0;
                    const active = index === activeIndex;
                    const Icon =
                      item.kind === 'game'
                        ? IconDeviceGamepad2
                        : item.kind === 'settings'
                          ? IconSettings
                          : item.icon;
                    const label =
                      item.kind === 'game' ? item.entry.title : item.label;
                    const subtitle =
                      item.kind === 'game' ? gameSubtitle(t, item.entry) : null;
                    const trailing =
                      item.kind === 'game'
                        ? t(GAME_ACTION_LABEL[item.action])
                        : null;
                    return (
                      <button
                        key={item.id}
                        ref={(element) => {
                          if (element) optionRefs.current.set(index, element);
                          else optionRefs.current.delete(index);
                        }}
                        id={`${baseId}-option-${index}`}
                        type="button"
                        role="option"
                        aria-selected={active}
                        data-testid={`palette-option-${index}`}
                        data-kind={item.kind}
                        data-key={item.kind === 'game' ? item.entry.key : undefined}
                        onMouseEnter={() => setActiveIndex(index)}
                        onClick={() => void runItem(item)}
                        className={cx(
                          'flex min-h-[40px] w-full items-center gap-[12px] rounded-control px-[10px] py-[3px] text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand',
                          active ? 'bg-raised' : 'bg-transparent hover:bg-raised',
                        )}
                      >
                        <Icon
                          size={16}
                          stroke={1.75}
                          aria-hidden="true"
                          className={cx(
                            'shrink-0',
                            active ? 'text-text-primary' : 'text-text-secondary',
                          )}
                        />
                        <span className="flex min-w-0 flex-1 flex-col">
                          <span
                            className={cx(
                              'truncate text-[13px]',
                              active
                                ? 'font-medium text-text-primary'
                                : 'font-normal text-text-default',
                            )}
                          >
                            {label}
                          </span>
                          {subtitle && (
                            <span className="truncate text-[11px] text-text-dimmed">
                              {subtitle}
                            </span>
                          )}
                        </span>
                        {trailing && (
                          <span className="shrink-0 text-[11px] text-text-dimmed">
                            {trailing}
                          </span>
                        )}
                      </button>
                    );
                  })}
                </div>
              ))}

              {flat.length === 0 && (
                <div
                  data-testid="palette-no-results"
                  className="px-[12px] py-[24px] text-center text-xs text-text-dimmed"
                >
                  {t('Palette::No results')}
                </div>
              )}
            </div>

            <div className="flex shrink-0 items-center gap-[12px] border-t border-border-subtle px-[16px] py-[10px] text-[11px] text-text-dimmed">
              <span className="flex items-center gap-[4px]">
                <Kbd>{t('Palette::Arrows')}</Kbd>
                {t('Palette::navigate')}
              </span>
              <span className="flex items-center gap-[4px]">
                <Kbd>Enter</Kbd>
                {t('Palette::open')}
              </span>
              <span className="flex items-center gap-[4px]">
                <KbdCombo keys={['Shift', 'F10']} />
                {t('Palette::game actions')}
              </span>
            </div>
          </div>
        </FocusTrap>
      </div>
    </OptionalPortal>
  );
};
