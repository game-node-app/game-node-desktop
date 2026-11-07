import { useContext } from 'react';
import { MantineContext, MantineProvider } from '@mantine/core';
import { AppModal } from '../../components/AppModal';
import { commands } from '../../api/commands';
import { useAppInfo, useAuth } from '../../api/useSnapshot';
import { openSettingsModal } from '../../store/uiStore';
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

function SyncExplanationDialog({
  onDismiss,
  className,
  backgroundSupported,
}: SyncExplanationDialogProps) {
  const { t } = useI18n();

  const handleDismiss = () => {
    commands.dismissSyncExplanation();
    onDismiss?.();
  };

  const handleOpenSettings = () => {
    commands.dismissSyncExplanation();
    openSettingsModal('account');
    onDismiss?.();
  };

  return (
    <AppModal.Root
      opened
      onClose={handleDismiss}
      centered
      size="520px"
      zIndex={1000}
      padding={0}
      transitionProps={{ duration: 0 }}
      className={className}
    >
      <AppModal.Overlay backgroundOpacity={0.7} />
      <AppModal.Content
        data-testid="sync-explanation-modal"
        classNames={{
          content:
            'flex flex-col gap-md rounded-md border border-border-subtle bg-secondary p-[24px] shadow-modal',
        }}
      >
        <AppModal.Title className="text-lg font-semibold text-text-default">
          {t('Sync with GameNode')}
        </AppModal.Title>

        <p className="text-sm leading-[1.5] text-text-dimmed">
          {t(
            'Sync is on. Store playtime totals and favorite changes are sent to your GameNode account. You can turn sync off in Settings and keep using your library.',
          )}
        </p>

        {backgroundSupported && (
          <p className="text-[13px] leading-[1.4] text-text-dimmed">
            {t(
              'Closing the window keeps GameNode running in the tray. Use Quit to exit.',
            )}
          </p>
        )}

        <div className="mt-compact flex items-center gap-sm">
          <button
            type="button"
            data-testid="sync-primary-button"
            onClick={handleDismiss}
            className="h-control rounded-sm bg-brand px-md text-sm font-medium text-on-primary hover:bg-brand-4"
          >
            {t('Got it')}
          </button>

          <button
            type="button"
            data-testid="sync-secondary-button"
            onClick={handleOpenSettings}
            className="h-control rounded-sm border border-border-subtle bg-transparent px-md text-sm font-medium text-text-default hover:bg-raised"
          >
            {t('Open settings')}
          </button>
        </div>
      </AppModal.Content>
    </AppModal.Root>
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
