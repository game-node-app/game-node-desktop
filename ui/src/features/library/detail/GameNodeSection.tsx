import { Menu } from '@mantine/core';
import {
  IconCheck,
  IconChevronDown,
  IconLink,
  IconPlus,
} from '@tabler/icons-react';
import {
  commands,
  type Actions,
  type CollectionCount,
  type DetailView,
  type Membership,
} from '../../../api';
import {
  Button,
  MenuSurface,
  Pill,
  SectionLabel,
} from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatReason, formatStatus } from '../formatters';
import { ENTRY_STATUSES, isInGameNodeLibrary, membershipStatus } from './format';

export interface GameNodeSectionProps {
  detail: DetailView;
  actions: Actions | undefined;
  membership: Membership;
  collections: CollectionCount[];
  gameId: number | null;
}

/** The "On GameNode" section: status select, collections, Add or Edit on the website. */
export function GameNodeSection({
  detail,
  actions,
  membership,
  collections,
  gameId,
}: GameNodeSectionProps) {
  const { t } = useI18n();
  const row = detail.row;
  const inLibrary = isInGameNodeLibrary(membership);
  const status = membershipStatus(membership);
  const collectionIds = inLibrary ? row.gamenode?.collectionIds ?? [] : [];
  const collectionNames = collectionIds
    .map((id) => collections.find((collection) => collection.id === id)?.name)
    .filter((name): name is string => Boolean(name));
  const addReady = actions?.addToGamenode === 'available';
  const addReason =
    actions && actions.addToGamenode !== 'available'
      ? formatReason(actions.addToGamenode.disabled, t)
      : '';

  const editOnGameNode = () => {
    if (gameId != null) commands.openGameNodePage({ game: { id: gameId } });
  };

  return (
    <section data-testid="detail-gamenode" className="flex flex-col gap-[8px]">
      <SectionLabel
        action={
          gameId != null && inLibrary ? (
            <button
              type="button"
              data-testid="detail-edit-gamenode"
              onClick={editOnGameNode}
              className="rounded-xs text-xs font-medium text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              {t('Detail::Edit on GameNode')}
            </button>
          ) : undefined
        }
      >
        {t('Detail::On GameNode')}
      </SectionLabel>

      {inLibrary ? (
        <>
          <div className="flex items-center gap-[8px] border-b border-border-subtle py-[6px]">
            <span className="text-[13px] text-text-secondary">{t('Detail::Status')}</span>
            <span className="flex-1" />
            {status != null ? (
              <MenuSurface position="bottom-end" width={180} withinPortal hideDetached={false} transitionProps={{ duration: 0 }}>
                <Menu.Target>
                  <button
                    type="button"
                    data-testid="detail-status-select"
                    aria-label={t('Detail::Status')}
                    className="inline-flex h-[26px] items-center gap-[6px] rounded-control border border-border-subtle bg-raised px-[8px] text-xs font-medium text-text-primary transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                  >
                    {formatStatus(status, t)}
                    <IconChevronDown size={12} aria-hidden="true" className="text-text-dimmed" />
                  </button>
                </Menu.Target>
                <Menu.Dropdown>
                  {ENTRY_STATUSES.map((option) => (
                    <Menu.Item
                      key={option}
                      role="menuitemradio"
                      aria-checked={option === status}
                      leftSection={
                        option === status ? (
                          <IconCheck size={16} stroke={2} aria-hidden="true" />
                        ) : (
                          <span className="block w-[16px]" />
                        )
                      }
                      onClick={() =>
                        commands.libraryInput({
                          setStatus: { keys: [row.key], status: option },
                        })
                      }
                    >
                      {formatStatus(option, t)}
                    </Menu.Item>
                  ))}
                </Menu.Dropdown>
              </MenuSurface>
            ) : (
              <Pill tone="neutral">{t('Detail::In your GameNode library')}</Pill>
            )}
          </div>

          <div className="flex items-center gap-[8px] py-[6px]">
            <span className="text-[13px] text-text-secondary">
              {t('Detail::Collections')}
            </span>
            <span className="flex-1" />
            <div className="flex flex-wrap items-center justify-end gap-[4px]">
              {collectionNames.length > 0 ? (
                collectionNames.map((name) => (
                  <Pill key={name} tone="neutral">
                    {name}
                  </Pill>
                ))
              ) : (
                <span className="text-[13px] text-text-dimmed">{t('Detail::None')}</span>
              )}
              <MenuSurface position="bottom-end" width={200} withinPortal hideDetached={false} transitionProps={{ duration: 0 }}>
                <Menu.Target>
                  <button
                    type="button"
                    data-testid="detail-collection-add"
                    aria-label={t('Library::Add to collection')}
                    className="inline-flex h-[18px] items-center rounded-full bg-raised px-[8px] text-xs font-medium leading-none text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                  >
                    <IconPlus size={12} stroke={2} aria-hidden="true" />
                  </button>
                </Menu.Target>
                <Menu.Dropdown>
                  {collections.length === 0 ? (
                    <Menu.Item disabled>{t('Library::No collections')}</Menu.Item>
                  ) : (
                    collections.map((collection) => (
                      <Menu.Item
                        key={collection.id}
                        onClick={() =>
                          commands.libraryInput({
                            addToCollection: {
                              keys: [row.key],
                              collectionId: collection.id,
                            },
                          })
                        }
                      >
                        {collection.name}
                      </Menu.Item>
                    ))
                  )}
                </Menu.Dropdown>
              </MenuSurface>
            </div>
          </div>

          <div className="flex items-center gap-[8px] border-b border-border-subtle py-[6px] last:border-b-0">
            <IconLink size={14} stroke={2} aria-hidden="true" className="text-text-dimmed" />
            <span className="text-[13px] text-text-secondary">
              {t('Detail::Matched')}
            </span>
            <span className="flex-1" />
            <span className="truncate text-[13px] font-medium text-text-default">
              {row.matchedName ?? row.title}
            </span>
          </div>
        </>
      ) : (
        <div className="flex flex-col gap-[8px]">
          <div data-testid="detail-membership-standing" className="text-[13px] text-text-dimmed">
            {t('Detail::Not in your GameNode library')}
          </div>
          <div>
            <Button
              variant="secondary"
              fullWidth
              data-testid="detail-add-btn"
              disabled={!addReady}
              leftIcon={IconPlus}
              onClick={() => commands.libraryInput('openAdd')}
            >
              {t('Library::Add to GameNode…')}
            </Button>
            {!addReady && addReason && (
              <div data-testid="detail-add-reason" className="mt-[4px] text-xs text-warning">
                {addReason}
              </div>
            )}
          </div>
          {actions?.addToGamenode !== 'available' &&
            typeof actions?.addToGamenode === 'object' &&
            actions.addToGamenode.disabled === 'membershipUnavailable' && (
              <button
                type="button"
                data-testid="detail-refresh-membership-btn"
                onClick={() => commands.libraryInput('refreshMembership')}
                className="self-start rounded-xs border border-border-subtle bg-transparent px-[8px] py-1 text-xs text-text-default transition-colors hover:bg-surface"
              >
                {t('Library::Check again')}
              </button>
            )}
        </div>
      )}
    </section>
  );
}
