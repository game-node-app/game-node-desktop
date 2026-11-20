import React, { useRef } from 'react';
import {
  IconChevronRight,
  IconFolder,
  IconLink,
  type TablerIcon,
} from '@tabler/icons-react';
import { ConfirmDialog } from '../../components/dialog';
import { Button } from '../../components/ui';
import { formatLongPlaytime } from '../../components/ui/gamePresentation';
import {
  commands,
  coverAssetUrl,
  useLibraryView,
  useLocalGames,
  type PendingClaim,
} from '../../api';
import { useI18n } from '../../i18n';
import { providerLabel } from '../../i18n/providerLabel';
import { providerIcon } from '../library/libraryControlIcons';

export interface ClaimConflictModalProps {
  claim?: PendingClaim | null;
  onClose?: () => void;
}

/**
 * The store glyph for the conflict card. Steam uses its brand mark; other
 * manifests keep their own library icon instead of borrowing Steam's.
 */
export function claimProviderIcon(provider: string): TablerIcon {
  return providerIcon(provider);
}

const ClaimCover: React.FC<{ file?: string | null; testId: string }> = ({
  file,
  testId,
}) => {
  const src = coverAssetUrl(file);
  return (
    <div
      data-testid={testId}
      className="h-[37px] w-[28px] shrink-0 overflow-hidden rounded-thumb bg-surface"
    >
      {src ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          className="block h-full w-full object-cover"
        />
      ) : (
        <div aria-hidden="true" className="h-full w-full bg-raised" />
      )}
    </div>
  );
};

/**
 * D11: the tracking conflict shows both registrations side by side before the
 * choice. Moving tracking is the only primary action; Cancel keeps the store
 * entry tracking and does not save the DRM-free registration.
 */
export const ClaimConflictModal: React.FC<ClaimConflictModalProps> = ({
  claim: propClaim,
  onClose,
}) => {
  const { t } = useI18n();
  const localGames = useLocalGames();
  const library = useLibraryView();
  const cancelRef = useRef<HTMLButtonElement>(null);

  const claim = propClaim !== undefined ? propClaim : localGames.pendingClaim;
  const isOpen = claim != null;

  const handleCancel = () => {
    commands.cancelLocalClaim();
    onClose?.();
  };

  const handleResolve = (transfer: boolean) => {
    commands.resolveLocalClaim(transfer);
    onClose?.();
  };

  if (!isOpen || !claim) {
    return null;
  }

  const provider = claim.storeEntry.provider;
  const ProviderIcon = claimProviderIcon(provider);
  const providerName = providerLabel(
    provider,
    library?.providers.find((item) => item.id === provider)?.name,
    t,
  );
  const playtime =
    claim.storeEntry.playtimeSeconds != null
      ? formatLongPlaytime(claim.storeEntry.playtimeSeconds, t)
      : null;
  const storeMeta = playtime ? `${providerName} · ${playtime}` : providerName;

  return (
    <ConfirmDialog
      tone="warning"
      icon={IconLink}
      size={520}
      title={t('drmFree.thisGameIsAlreadyTrackedByA')}
      description={t('drmFree.useTheExistingEntryOrTransferTracking')}
      initialFocusRef={cancelRef}
      onClose={handleCancel}
      testId="claim-conflict-dialog"
      actions={
        <>
          <Button
            ref={cancelRef}
            variant="ghost"
            size="sm"
            data-testid="claim-cancel"
            onClick={handleCancel}
          >
            {t('drmFree.cancel')}
          </Button>
          <span aria-hidden="true" className="flex-1" />
          <Button
            variant="secondary"
            size="sm"
            data-testid="claim-resolve-separate"
            onClick={() => handleResolve(false)}
          >
            {t('drmFree.useExistingEntry')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            data-testid="claim-resolve-transfer"
            onClick={() => handleResolve(true)}
          >
            {t('drmFree.transferTracking')}
          </Button>
        </>
      }
    >
      <div className="flex items-center gap-[10px]">
        <div
          data-testid="claim-store-card"
          data-provider={provider}
          className="flex min-w-0 flex-1 items-center gap-compact rounded-md border border-border-subtle bg-body p-compact"
        >
          <ClaimCover file={claim.storeEntry.coverFile} testId="claim-store-cover" />
          <div className="flex min-w-0 flex-col gap-[2px]">
            <span className="truncate text-body font-semibold text-text-primary">
              {claim.storeEntry.title}
            </span>
            <span className="flex min-w-0 items-center gap-1 text-caption text-text-dimmed">
              <span
                data-testid="claim-store-provider-icon"
                data-provider={provider}
                className="shrink-0"
              >
                <ProviderIcon size={12} stroke={2} aria-hidden="true" />
              </span>
              <span className="truncate">{storeMeta}</span>
            </span>
          </div>
        </div>

        <IconChevronRight
          size={16}
          stroke={2}
          aria-hidden="true"
          className="shrink-0 text-text-muted"
        />

        <div
          data-testid="claim-registration-card"
          className="flex min-w-0 flex-1 items-center gap-compact rounded-md border border-border-subtle bg-body p-compact"
        >
          <ClaimCover
            file={claim.registration.coverFile}
            testId="claim-registration-cover"
          />
          <div className="flex min-w-0 flex-col gap-[2px]">
            <span className="truncate text-body font-semibold text-text-primary">
              {claim.registration.title}
            </span>
            <span className="flex min-w-0 items-center gap-1 text-caption text-text-dimmed">
              <IconFolder
                size={12}
                stroke={2}
                aria-hidden="true"
                className="shrink-0"
              />
              <span className="truncate">
                {t('drmFree.drmFreeWithPath', { path: claim.registration.root })}
              </span>
            </span>
          </div>
        </div>
      </div>
    </ConfirmDialog>
  );
};
