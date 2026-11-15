import React from 'react';
import { IconLoader2, IconPlus, IconRefresh } from '@tabler/icons-react';
import {
  commands,
  useLibraryView,
  type LibraryView,
  type Reason,
} from '../../api';
import { Button, Tooltip } from '../../components/ui';
import { useI18n } from '../../i18n';
import { formatI18n, formatReason } from './formatters';
import {
  groupByLabel,
  titleSourceLabel,
  type Translate,
} from './libraryControlLabels';

function subtitle(library: LibraryView, t: Translate): string {
  const total = library.total;
  const installed = library.installedTotal;
  const stores = library.providers.length;
  const selected = library.selected.length;
  const group = library.layout.group;

  if (group !== 'none') {
    return formatI18n(
      t('Library::{} games · grouped by {} · {} selected'),
      total,
      groupByLabel(group, t),
      selected,
    );
  }

  const normal = formatI18n(
    t('Library::{} games · {} installed on this PC · {} stores'),
    total,
    installed,
    stores,
  );
  if (selected > 0) {
    return `${normal} · ${formatI18n(t('Library::{} selected'), selected)}`;
  }
  return normal;
}

/**
 * Library title, live subtitle and the two header actions. Standalone and
 * store-connected so the integration screen only mounts `<LibraryHeader />`;
 * it never mounts the toolbar or the active-filter chips.
 */
export const LibraryHeader: React.FC = () => {
  const library = useLibraryView();
  const { t } = useI18n();

  const title = titleSourceLabel(library?.titleSource, library, t);
  const description = library ? subtitle(library, t) : '';
  const scanning = library?.scanning ?? false;
  const addAvailability = library?.actions.addToGamenode ?? {
    disabled: 'nothingSelected' as Reason,
  };
  const addDisabled = addAvailability !== 'available';
  const addReason =
    typeof addAvailability === 'object' ? formatReason(addAvailability.disabled, t) : '';
  const reasonId = 'library-add-disabled-reason';

  const addButton = (
    <Button
      variant="primary"
      size="sm"
      leftIcon={IconPlus}
      data-testid="library-header-add"
      disabled={addDisabled}
      aria-describedby={addDisabled ? reasonId : undefined}
      onClick={() => commands.libraryInput('openAdd')}
    >
      {t('Library::Add to GameNode')}
    </Button>
  );

  return (
    <header
      data-testid="library-header"
      className="flex shrink-0 items-center gap-[8px] px-[24px] pb-[14px] pt-[20px]"
    >
      <div className="min-w-0 flex-1">
        <h1
          data-testid="library-header-title"
          className="truncate text-heading font-semibold leading-tight text-text-primary"
        >
          {title}
        </h1>
        <p
          data-testid="library-header-subtitle"
          className="mt-[2px] truncate text-body font-normal text-text-dimmed"
        >
          {description}
        </p>
      </div>

      <Button
        variant="ghost"
        size="sm"
        leftIcon={scanning ? IconLoader2 : IconRefresh}
        data-testid="library-header-rescan"
        data-scanning={scanning ? 'true' : 'false'}
        aria-busy={scanning || undefined}
        className={scanning ? '[&_svg]:animate-spin' : undefined}
        onClick={() => commands.rescanLibrary()}
      >
        {t('Library::Rescan')}
      </Button>

      {addDisabled ? (
        <>
          <Tooltip label={addReason}>
            <span className="inline-flex">{addButton}</span>
          </Tooltip>
          <span id={reasonId} className="sr-only">
            {addReason}
          </span>
        </>
      ) : (
        addButton
      )}
    </header>
  );
};
