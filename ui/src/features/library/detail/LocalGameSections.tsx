import { IconChevronRight, IconTrash } from '@tabler/icons-react';
import {
  commands,
  type DetailView,
  type GameToolView,
  type Registration,
  type SaveProfile,
} from '../../../api';
import { KeyValueRow, Pill, SectionLabel } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { uiStore } from '../../../store/uiStore';
import { entryRegistrationId, toolSummary } from './format';

export interface LocalGameSectionsProps {
  detail: DetailView;
  registration: Registration | undefined;
  profile: SaveProfile | undefined;
  tools: GameToolView[];
  isRunning: boolean;
  protectionActive: boolean;
}

/** DRM-free launch configuration plus the tools and save-protection section. */
export function LocalGameSections({
  detail,
  registration,
  profile,
  tools,
  isRunning,
  protectionActive,
}: LocalGameSectionsProps) {
  const { t } = useI18n();
  const row = detail.row;
  const id = entryRegistrationId(row.key);
  const installedTools = toolSummary(tools);
  const executable = registration?.executable ?? '';
  const argumentsText = registration?.arguments?.join(' ') ?? '';
  const saveFolder = profile?.files?.[0] ?? '';

  const openEditor = () =>
    uiStore.setState((prev) => ({ ...prev, localEditorEntryKey: row.key }));

  const openProtection = () => {
    uiStore.setState((prev) => ({ ...prev, saveProtectionEntryKey: row.key }));
    commands.loadLocalHistory(id);
  };

  return (
    <div className="flex flex-col gap-[18px]">
      <section data-testid="detail-game-settings" className="flex flex-col gap-[6px]">
        <SectionLabel
          action={
            <button
              type="button"
              data-testid="detail-edit-game-btn"
              onClick={openEditor}
              className="rounded-xs text-xs font-medium text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              {t('Detail::Edit…')}
            </button>
          }
        >
          {t('Detail::Game settings')}
        </SectionLabel>

        <KeyValueRow
          label={t('Detail::Executable')}
          value={
            executable ? (
              <span data-testid="detail-executable" title={executable}>
                {executable}
              </span>
            ) : undefined
          }
          emptyLabel={t('Detail::None')}
        />
        <KeyValueRow
          label={t('Detail::Arguments')}
          value={
            argumentsText ? (
              <span data-testid="detail-arguments" title={argumentsText}>
                {argumentsText}
              </span>
            ) : undefined
          }
          emptyLabel={t('Detail::None')}
        />
        <KeyValueRow
          label={t('Detail::Save folder')}
          value={
            saveFolder ? (
              <span data-testid="detail-save-folder" title={saveFolder}>
                {saveFolder}
              </span>
            ) : undefined
          }
          emptyLabel={t('Detail::None')}
          last
        />
      </section>

      <section data-testid="detail-tools-protection" className="flex flex-col gap-[6px]">
        <SectionLabel>{t('Detail::Tools and protection')}</SectionLabel>

        <button
          type="button"
          data-testid="detail-save-protection-btn"
          onClick={openProtection}
          className="flex w-full items-center gap-[8px] border-b border-border-subtle py-[6px] text-left transition-colors hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          <span className="shrink-0 text-[13px] text-text-secondary">
            {t('Detail::Local save protection')}
          </span>
          <span className="flex-1" />
          {protectionActive ? (
            <Pill tone="success">{t('Detail::Active')}</Pill>
          ) : (
            <span className="text-[13px] text-text-dimmed">{t('Detail::Off')}</span>
          )}
          <IconChevronRight
            size={14}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-dimmed"
          />
        </button>

        <div data-testid="detail-tools-btn">
          <KeyValueRow
            label={t('Detail::Game tools')}
            value={
              installedTools ? (
                <span data-testid="detail-tools-summary">
                  <Pill tone="success">{installedTools}</Pill>
                </span>
              ) : (
                <span className="text-[13px] text-text-dimmed">{t('Detail::None')}</span>
              )
            }
            onClick={isRunning ? undefined : () => commands.openTools(row.key)}
            chevron
            last
          />
        </div>
        {isRunning && (
          <div className="text-xs text-warning">{t('Library::Running')}</div>
        )}
      </section>

      <button
        type="button"
        data-testid="detail-remove-btn"
        onClick={() =>
          uiStore.setState((prev) => ({ ...prev, removeModalEntryKey: row.key }))
        }
        className="flex items-center gap-[6px] self-start rounded-xs text-[13px] font-medium text-danger transition-colors hover:text-danger/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger"
      >
        <IconTrash size={14} stroke={2} aria-hidden="true" />
        {t('Detail::Remove from library…')}
      </button>
    </div>
  );
}
