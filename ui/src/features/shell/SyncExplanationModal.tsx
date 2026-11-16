import { useContext, useRef, useState } from 'react';
import { MantineContext, MantineProvider } from '@mantine/core';
import { IconCloudCheck } from '@tabler/icons-react';
import { ConfirmDialog } from '../../components/dialog';
import { Button, Checklist } from '../../components/ui';
import { commands } from '../../api/commands';
import { useAppInfo, useAuth } from '../../api/useSnapshot';
import { useI18n } from '../../i18n';
import { appTheme } from '../../AppProviders';

export interface SyncExplanationModalProps {
  forceOpen?: boolean;
  onDismiss?: () => void;
  className?: string;
}

interface SyncExplanationDialogProps extends SyncExplanationModalProps {
  backgroundSupported: boolean;
}

/**
 * D12: first-use explanation of what sync sends and what stays on this PC.
 * The list mirrors `desktop-sync`; statuses and collections are sent only
 * through explicit actions, so they are not listed here.
 */
function SyncExplanationDialog({
  onDismiss,
  className,
  backgroundSupported,
}: SyncExplanationDialogProps) {
  const { t } = useI18n();
  const keepSyncRef = useRef<HTMLButtonElement>(null);
  const [turningOff, setTurningOff] = useState(false);

  const dismiss = () => {
    void commands.dismissSyncExplanation();
    onDismiss?.();
  };

  const handleTurnSyncOff = async () => {
    if (turningOff) return;
    setTurningOff(true);
    try {
      await commands.setSyncEnabled(false);
      await commands.dismissSyncExplanation();
      onDismiss?.();
    } catch {
      // Keep the explanation blocking writes when sync could not be disabled:
      // dismissing it here would let queued playtime and favorites leave.
    } finally {
      setTurningOff(false);
    }
  };

  return (
    <ConfirmDialog
      tone="success"
      icon={IconCloudCheck}
      size={480}
      title={t('app.playtimeSync')}
      description={t('app.syncExplanationDescription')}
      initialFocusRef={keepSyncRef}
      onClose={dismiss}
      testId="sync-explanation-modal"
      className={className}
      actions={
        <>
          <Button
            variant="ghost"
            size="sm"
            loading={turningOff}
            data-testid="sync-turn-off-button"
            onClick={() => void handleTurnSyncOff()}
          >
            {t('app.turnSyncOff')}
          </Button>
          <span aria-hidden="true" className="flex-1" />
          <Button
            ref={keepSyncRef}
            variant="primary"
            size="sm"
            data-testid="sync-primary-button"
            onClick={dismiss}
          >
            {t('app.keepSyncOn')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-compact">
        <Checklist
          variant="inset"
          items={[
            { kind: 'positive', label: t('app.syncSendsPlaytime') },
            { kind: 'positive', label: t('app.syncSendsDetectedGames') },
            { kind: 'positive', label: t('app.syncSendsFavorites') },
            { kind: 'negative', label: t('app.syncStaysSaveFiles') },
            { kind: 'negative', label: t('app.syncStaysInstallPaths') },
          ]}
        />
        {backgroundSupported && (
          <p
            data-testid="sync-tray-note"
            className="text-xs leading-note text-text-dimmed"
          >
            {t('app.closingTheWindowKeepsGamenodeRunningIn')}
          </p>
        )}
      </div>
    </ConfirmDialog>
  );
}

export function SyncExplanationModal({
  forceOpen,
  onDismiss,
  className,
}: SyncExplanationModalProps = {}) {
  const { auth, sync } = useAuth();
  const appInfo = useAppInfo();
  const mantineContext = useContext(MantineContext);

  const isSignedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const isOpen =
    forceOpen !== undefined ? forceOpen : isSignedIn && !sync?.explained;

  if (!isOpen) {
    return null;
  }

  const backgroundSupported = appInfo?.data?.backgroundSupported ?? false;
  const dialog = (
    <SyncExplanationDialog
      onDismiss={onDismiss}
      className={className}
      backgroundSupported={backgroundSupported}
    />
  );

  // Mantine v9 primitives require a provider context. The app mounts one in
  // AppProviders; keep standalone renders (component tests) working as well.
  if (mantineContext) {
    return dialog;
  }

  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      {dialog}
    </MantineProvider>
  );
}
