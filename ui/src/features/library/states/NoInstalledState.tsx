import { IconDeviceDesktop } from '@tabler/icons-react';
import { Button, EmptyState } from '../../../components/ui';
import { useI18n } from '../../../i18n';

export interface NoInstalledStateProps {
  onShowAll: () => void;
}

/** The Installed scope or availability fact hides every entry while games exist. */
export function NoInstalledState({ onShowAll }: NoInstalledStateProps) {
  const { t } = useI18n();

  return (
    <div
      data-testid="library-empty-state"
      className="flex flex-1 items-center justify-center p-[24px]"
    >
      <EmptyState
        icon={IconDeviceDesktop}
        title={t('Library::Nothing is installed on this PC.')}
        body={t(
          'Library::Games you own or share are hidden by the Installed filter.',
        )}
        actions={
          <Button
            variant="primary"
            size="sm"
            data-testid="library-empty-show-all-btn"
            onClick={onShowAll}
          >
            {t('Library::Show all games')}
          </Button>
        }
      />
    </div>
  );
}
