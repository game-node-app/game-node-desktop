import { IconLoader2 } from '@tabler/icons-react';
import type { ScanProgress } from '../../../api';
import { Skeleton } from '../../../components/ui';
import { useI18n } from '../../../i18n';

export interface ScanningPanelProps {
  /** Engine-provided provider rows, including the DRM-free folders pseudo row. */
  progress: ScanProgress[];
  /** Show the 12 tile-shaped skeletons while the first scan has no rows yet. */
  showSkeletons?: boolean;
}

const TRACK_CLASS: Record<ScanProgress['state'], string> = {
  waiting: 'w-0 bg-raised',
  scanning: 'w-1/3 bg-brand',
  done: 'w-full bg-success',
  failed: 'w-full bg-danger',
};

/**
 * Per-provider scanning progress from D12. The panel sits above the grid so
 * known rows stay usable; before the first rows arrive it is followed by the
 * same skeleton geometry as the grid tiles.
 */
export function ScanningPanel({ progress, showSkeletons = false }: ScanningPanelProps) {
  const { t } = useI18n();

  const label = (row: ScanProgress) =>
    row.provider === 'drm-free' ? t('Library::DRM-free folders') : row.name;

  return (
    <div
      data-testid="library-scanning-panel"
      className={showSkeletons ? 'flex min-h-0 flex-1 flex-col' : 'shrink-0'}
    >
      <div className="flex flex-col gap-[10px] border-b border-border-subtle bg-surface p-[12px]">
        <div className="flex items-center gap-[10px]">
          <IconLoader2
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 animate-spin text-brand"
          />
          <span className="min-w-0 flex-1 text-[13px] font-semibold text-text-primary">
            {t('Library::Scanning the library…')}
          </span>
        </div>

        {progress.length > 0 ? (
          <div className="flex flex-col gap-[8px]">
            {progress.map((row) => (
              <div
                key={row.provider}
                data-testid={`scan-progress-${row.provider}`}
                data-state={row.state}
                className="flex items-center gap-[10px]"
              >
                <span className="w-[120px] shrink-0 truncate text-xs font-normal text-text-secondary">
                  {label(row)}
                </span>
                <span
                  aria-hidden="true"
                  className="h-1 min-w-0 flex-1 overflow-hidden rounded-sm bg-raised"
                >
                  <span
                    className={`block h-full rounded-sm ${
                      TRACK_CLASS[row.state]
                    } ${row.state === 'scanning' ? 'animate-pulse' : ''}`}
                  />
                </span>
                <span className="w-10 shrink-0 text-right text-xs tabular-nums text-text-dimmed">
                  {row.state === 'failed'
                    ? t('Library::Failed')
                    : row.state === 'scanning'
                      ? '…'
                      : row.found ?? ''}
                </span>
              </div>
            ))}
          </div>
        ) : null}
      </div>

      {showSkeletons ? (
        <div
          data-testid="library-scanning-skeletons"
          className="grid flex-1 grid-cols-[repeat(auto-fill,minmax(172px,1fr))] content-start gap-[16px] px-[24px] pb-[24px] pt-[16px]"
        >
          {Array.from({ length: 12 }, (_, index) => (
            <div key={index} className="flex flex-col gap-[8px]">
              <Skeleton className="h-[214px] w-full rounded-control" />
              <Skeleton className="h-2.5 w-[120px]" />
              <Skeleton className="h-2 w-[80px]" />
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
