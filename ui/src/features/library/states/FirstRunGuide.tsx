import React from 'react';
import { IconBooks, IconCheck } from '@tabler/icons-react';
import { Button, EmptyState } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { formatI18n } from '../formatters';

export interface FirstRunGuideProps {
  signedIn: boolean;
  username: string | null;
  storeConnected: boolean;
  folderAdded: boolean;
  onConnect: () => void;
  onChooseFolder: () => void;
  onRescan: () => void;
}

interface ChecklistStepProps {
  done: boolean;
  index: number;
  title: string;
  detail: string;
  action?: React.ReactNode;
}

function ChecklistStep({ done, index, title, detail, action }: ChecklistStepProps) {
  return (
    <li
      data-testid={`first-run-step-${index}`}
      data-done={done ? 'true' : 'false'}
      className="flex items-center gap-[12px] rounded-control p-[10px]"
    >
      <span
        aria-hidden="true"
        className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
          done ? 'bg-success text-text-primary' : 'bg-raised text-text-secondary'
        }`}
      >
        {done ? <IconCheck size={14} stroke={2} /> : index}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-[2px]">
        <span
          className={`text-[13px] font-medium ${
            done ? 'text-text-dimmed' : 'text-text-primary'
          }`}
        >
          {title}
        </span>
        <span className="text-[11px] font-normal text-text-dimmed">{detail}</span>
      </div>
      {!done && action ? <div className="shrink-0">{action}</div> : null}
    </li>
  );
}

/**
 * First-run guidance from `raw/07-L5-states.txt`: the three-step checklist names
 * sign-in, a store connection and a DRM-free folder, marks each one done and
 * offers only the first pending action as primary. Rescan stays available once
 * the scan has found nothing.
 */
export function FirstRunGuide({
  signedIn,
  username,
  storeConnected,
  folderAdded,
  onConnect,
  onChooseFolder,
  onRescan,
}: FirstRunGuideProps) {
  const { t } = useI18n();
  const name = username?.trim() || '';
  // Step 1 has no action: the library is only shown with a session. The first
  // pending store/folder action is the one Primary button.
  const firstPending = !storeConnected ? 1 : !folderAdded ? 2 : -1;

  const stepAction = (step: number, label: string, onClick: () => void) => (
    <Button
      variant={firstPending === step ? 'primary' : 'secondary'}
      size="sm"
      data-testid={`first-run-action-${step + 1}`}
      onClick={onClick}
    >
      {label}
    </Button>
  );

  return (
    <div
      data-testid="library-first-run"
      className="flex flex-1 flex-col items-center justify-center gap-[20px] p-[24px] text-center"
    >
      <EmptyState
        icon={IconBooks}
        title={t('Library::Your library is empty')}
        body={t(
          'Library::Connect a store or choose a DRM-free games folder to get started.',
        )}
        size="lg"
      />

      <ol className="flex w-[420px] max-w-full flex-col gap-[2px] rounded-md bg-secondary p-[6px]">
        <ChecklistStep
          index={1}
          done={signedIn}
          title={t('Library::Sign in to GameNode')}
          detail={
            signedIn && name
              ? formatI18n(t('Library::Account {} connected'), name)
              : ''
          }
        />
        <ChecklistStep
          index={2}
          done={storeConnected}
          title={t('Library::Connect a store')}
          detail={t('Library::Steam or Epic Games Store')}
          action={
            storeConnected
              ? undefined
              : stepAction(1, t('Library::Connect'), onConnect)
          }
        />
        <ChecklistStep
          index={3}
          done={folderAdded}
          title={t('Library::Add a DRM-free folder')}
          detail={t('Library::GameNode finds the executables')}
          action={
            folderAdded
              ? undefined
              : stepAction(2, t('Library::Choose folder…'), onChooseFolder)
          }
        />
      </ol>

      <Button variant="ghost" size="sm" data-testid="first-run-rescan" onClick={onRescan}>
        {t('Library::Rescan')}
      </Button>
    </div>
  );
}
