import { IconChevronRight } from '@tabler/icons-react';
import { commands, type DetailView, type GameToolView } from '../../../api';
import { KeyValueRow, Pill, SectionLabel } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { uiStore } from '../../../store/uiStore';
import { formatI18n } from '../formatters';
import { toolSummary } from './format';

export interface PortProvenanceSummary {
  portName: string;
  installedVersion: string;
  catalogId: string;
}

export interface OnThisPcSectionProps {
  detail: DetailView;
  isDrmFree: boolean;
  installPath: string | null;
  tools: GameToolView[];
  isRunning: boolean;
  portProvenance: PortProvenanceSummary | null;
}

/** The On this PC section: install folder, playtime counter, game tools and PC ports provenance. */
export function OnThisPcSection({
  detail,
  isDrmFree,
  installPath,
  tools,
  isRunning,
  portProvenance,
}: OnThisPcSectionProps) {
  const { t } = useI18n();
  const row = detail.row;
  const installedTools = toolSummary(tools);
  const counter =
    row.playtime.source === 'store' ? row.providerName : t('Detail::Internal');

  return (
    <section data-testid="detail-on-this-pc" className="flex flex-col gap-[6px]">
      <SectionLabel>{t('Detail::On this PC')}</SectionLabel>

      {row.installed && installPath ? (
        <button
          type="button"
          onClick={() => void commands.openInstallFolder(installPath)}
          className="flex w-full items-center gap-[8px] border-b border-border-subtle py-[6px] text-left transition-colors hover:bg-raised focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          <span className="shrink-0 text-[13px] text-text-secondary">
            {t('Detail::Install folder')}
          </span>
          <span className="flex-1" />
          <span
            data-testid="detail-install-dir"
            className="min-w-0 truncate text-[13px] font-medium text-text-default"
            title={installPath}
          >
            {installPath}
          </span>
          <IconChevronRight
            size={14}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-dimmed"
          />
        </button>
      ) : (
        <div
          data-testid="detail-not-installed"
          className="border-b border-border-subtle py-[6px] text-[13px] text-text-dimmed"
        >
          {t('Library::Not installed on this PC')}
        </div>
      )}

      {!isDrmFree && (
        <>
          <KeyValueRow
            label={t('Detail::Playtime counter')}
            value={<span data-testid="detail-playtime-counter">{counter}</span>}
          />
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
            />
          </div>
          {isRunning && (
            <div className="text-xs text-warning">{t('Library::Running')}</div>
          )}
        </>
      )}

      {portProvenance && (
        <div data-testid="detail-port-provenance" className="pt-[4px]">
          <div className="text-xs text-text-dimmed">
            {t('Library::Installed by PC ports')}
          </div>
          <div className="text-[13px] font-medium text-text-default">
            {formatI18n(
              t('Library::{} version {}'),
              portProvenance.portName,
              portProvenance.installedVersion,
            )}
          </div>
          <button
            type="button"
            data-testid="detail-port-link"
            onClick={() =>
              uiStore.setState((prev) => ({
                ...prev,
                activeDestination: 'ports',
                portsFocusCatalogId: portProvenance.catalogId,
              }))
            }
            className="mt-[4px] rounded-xs text-left text-xs text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('Library::Open in PC ports')}
          </button>
        </div>
      )}
    </section>
  );
}
