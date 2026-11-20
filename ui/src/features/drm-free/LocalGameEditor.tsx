import React, { useEffect, useRef, useState } from 'react';
import {
  IconCheck,
  IconChevronDown,
  IconFileText,
  IconFolder,
  IconInfoCircle,
  IconTerminal2,
  IconTrash,
} from '@tabler/icons-react';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  Callout,
  Field,
  SelectField,
  TextArea,
  TextInput,
} from '../../components/ui';
import {
  commands,
  coverAssetUrl,
  useLibraryView,
  useLocalGames,
  type Registration,
} from '../../api';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

export interface LocalGameEditorProps {
  registration?: Registration | null;
  onClose?: () => void;
}

export function parseArguments(input: string): string[] {
  if (!input.trim()) return [];
  const matches = input.match(/(?:[^\s"]+|"[^"]*")+/g);
  if (!matches) return [];
  return matches.map((arg) => (arg.startsWith('"') && arg.endsWith('"') ? arg.slice(1, -1) : arg));
}

function lines(value: string): string[] {
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
}

/** Whether the stored process detection differs from the executable derived by default. */
function hasCustomProcessValues(registration: Registration): boolean {
  const gameplayCustom =
    registration.gameplay.length > 0 &&
    !(
      registration.gameplay.length === 1 &&
      registration.gameplay[0] === registration.executable
    );
  return gameplayCustom || registration.launchers.length > 0;
}

function coverErrorKey(error: unknown): string {
  const code =
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : '';
  if (code === 'coverTooLarge') return 'drmFree.coverTooLarge';
  if (
    code === 'coverUnreadable' ||
    code === 'coverUnsupported' ||
    code === 'coverUnsupportedFormat'
  ) {
    return 'drmFree.coverUnreadable';
  }
  return 'drmFree.coverPickFailed';
}

export const LocalGameEditor: React.FC<LocalGameEditorProps> = ({
  registration: propRegistration,
  onClose,
}) => {
  const { t } = useI18n();
  const localGames = useLocalGames();
  const library = useLibraryView();
  const localEditorEntryKey = useUiStore((s) => s.localEditorEntryKey);

  const targetReg =
    propRegistration !== undefined
      ? propRegistration
      : localGames.registrations.find(
          (r) =>
            r.id === localEditorEntryKey ||
            `manual:${r.id}` === localEditorEntryKey ||
            (localEditorEntryKey && r.id === localEditorEntryKey.replace(/^manual:/, '')),
        ) ?? null;

  const isOpen = targetReg !== null;
  const entryKey = targetReg ? `manual:${targetReg.id}` : null;
  const matchedRow = entryKey
    ? library?.rows.find((row) => row.key === entryKey)
    : undefined;
  const gameNodeCover = matchedRow?.gameCoverFile ?? null;

  const [title, setTitle] = useState('');
  const [executable, setExecutable] = useState('');
  const [root, setRoot] = useState('');
  const [workingDirectory, setWorkingDirectory] = useState('');
  const [argsText, setArgsText] = useState('');
  const [gameplayText, setGameplayText] = useState('');
  const [launchersText, setLaunchersText] = useState('');
  const [pendingCover, setPendingCover] = useState<string | null>(null);
  const [clearedCover, setClearedCover] = useState(false);
  const [coverError, setCoverError] = useState<string | null>(null);
  const [pickingCover, setPickingCover] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const savedRef = useRef(false);
  const openRegistrationRef = useRef<string | null>(null);

  useEffect(() => {
    const current = targetReg?.id ?? null;
    if (
      openRegistrationRef.current != null &&
      openRegistrationRef.current !== current &&
      !savedRef.current
    ) {
      void commands.discardLocalCover(`manual:${openRegistrationRef.current}`);
    }
    openRegistrationRef.current = current;
  }, [targetReg?.id]);

  useEffect(
    () => () => {
      if (!savedRef.current && openRegistrationRef.current) {
        void commands.discardLocalCover(`manual:${openRegistrationRef.current}`);
      }
    },
    [],
  );

  useEffect(() => {
    if (targetReg) {
      setTitle(targetReg.title);
      setExecutable(targetReg.executable);
      setRoot(targetReg.root);
      setWorkingDirectory(targetReg.workingDirectory);
      setArgsText(targetReg.arguments.join(' '));
      setGameplayText(targetReg.gameplay.join('\n'));
      setLaunchersText(targetReg.launchers.join('\n'));
      setAdvancedOpen(hasCustomProcessValues(targetReg));
      setPendingCover(null);
      setClearedCover(false);
      setCoverError(null);
      setPickingCover(false);
      savedRef.current = false;
    }
  }, [targetReg?.id, targetReg?.revision]);

  const discardPending = () => {
    if (entryKey) {
      void commands.discardLocalCover(entryKey);
    }
  };

  const handleClose = () => {
    if (!savedRef.current) {
      discardPending();
    }
    uiStore.setState((s) => ({ ...s, localEditorEntryKey: null }));
    onClose?.();
  };

  const handleRemove = () => {
    discardPending();
    uiStore.setState((s) => ({
      ...s,
      localEditorEntryKey: null,
      removeModalEntryKey: entryKey,
    }));
    onClose?.();
  };

  if (!isOpen || !targetReg || !entryKey) {
    return null;
  }

  const matchingCandidate = localGames.candidates.find(
    (c) =>
      (targetReg.root && c.root.toLowerCase() === targetReg.root.toLowerCase()) ||
      c.targets.includes(targetReg.executable) ||
      (c.title && c.title.toLowerCase() === targetReg.title.toLowerCase()),
  );

  const handleCandidateChange = (newExe: string) => {
    if (gameplayText === executable || !gameplayText.trim()) {
      setGameplayText(newExe);
    }
    setExecutable(newExe);
  };

  const handleChangeCover = async () => {
    setCoverError(null);
    setPickingCover(true);
    try {
      const file = await commands.pickLocalCover(entryKey);
      if (file) {
        setPendingCover(file);
        setClearedCover(false);
      }
    } catch (error) {
      setCoverError(coverErrorKey(error));
    } finally {
      setPickingCover(false);
    }
  };

  const handleUseGameNodeCover = () => {
    discardPending();
    setPendingCover(null);
    setClearedCover(true);
    setCoverError(null);
  };

  const handleSave = () => {
    const committedCover = clearedCover ? null : targetReg.customCover ?? null;
    const customCover = pendingCover ?? committedCover;
    const updated: Registration = {
      ...targetReg,
      title: title.trim(),
      executable: executable.trim(),
      root: root.trim(),
      workingDirectory: workingDirectory.trim(),
      arguments: parseArguments(argsText),
      gameplay: lines(gameplayText),
      launchers: lines(launchersText),
      customCover,
    };

    savedRef.current = true;
    commands.editLocalGame(updated);
    handleClose();
  };

  const committedCover = clearedCover ? null : targetReg.customCover ?? null;
  const displayCover = pendingCover ?? committedCover ?? gameNodeCover;
  const coverSrc = coverAssetUrl(displayCover);
  const hasCustomCover = pendingCover !== null || committedCover !== null;
  const multipleExecutables = (matchingCandidate?.targets.length ?? 0) > 1;

  return (
    <DialogShell
      title={t('drmFree.gameSettings')}
      subtitle={t('drmFree.gameSettingsSubtitle', { game: targetReg.title })}
      onClose={handleClose}
      geometryKey="local-game-editor"
      defaultSize={{ width: 640, height: 660 }}
      minSize={{ width: 520, height: 480 }}
      testId="local-game-editor"
      footerLeading={
        <Button
          variant="ghost"
          size="sm"
          leftIcon={IconTrash}
          onClick={handleRemove}
          data-testid="local-editor-remove"
        >
          {t('drmFree.removeFromLibraryEllipsis')}
        </Button>
      }
      footer={
        <div className="flex items-center gap-compact">
          <Button variant="secondary" size="sm" onClick={handleClose}>
            {t('drmFree.cancel')}
          </Button>
          <Button
            variant="primary"
            size="sm"
            leftIcon={IconCheck}
            onClick={handleSave}
            data-testid="local-editor-save"
          >
            {t('drmFree.save')}
          </Button>
        </div>
      }
    >
      <div className="flex items-start gap-md">
        <div className="flex w-[108px] shrink-0 flex-col items-center gap-compact">
          <div
            data-testid="local-editor-cover"
            className="h-[144px] w-[108px] overflow-hidden rounded-control border border-border-subtle bg-surface"
          >
            {coverSrc ? (
              <img
                data-testid="local-editor-cover-img"
                src={coverSrc}
                alt={targetReg.title}
                loading="lazy"
                decoding="async"
                className="h-full w-full object-cover"
              />
            ) : (
              <div
                data-testid="local-editor-cover-placeholder"
                aria-hidden="true"
                className="h-full w-full bg-gradient-to-br from-brand/30 via-secondary to-canvas"
              />
            )}
          </div>
          <button
            type="button"
            data-testid="local-editor-change-cover"
            onClick={handleChangeCover}
            disabled={pickingCover}
            className="text-xs font-medium text-brand transition-colors hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t('drmFree.changeCover')}
          </button>
          <button
            type="button"
            data-testid="local-editor-use-gamenode-cover"
            onClick={handleUseGameNodeCover}
            disabled={!hasCustomCover}
            className="text-center text-xs font-medium text-text-secondary transition-colors hover:text-text-default hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t('drmFree.useGameNodeCover')}
          </button>
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-md">
          <Field label={t('drmFree.title')}>
            <TextInput
              data-testid="local-editor-title"
              aria-label={t('drmFree.title')}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
            />
          </Field>

          <div className="flex items-end gap-compact">
            <Field label={t('drmFree.executable')} className="min-w-0 flex-1">
              <TextInput
                data-testid="local-editor-executable"
                aria-label={t('drmFree.executable')}
                leadingIcon={IconFileText}
                value={executable}
                onChange={(event) => setExecutable(event.target.value)}
              />
            </Field>
            <Button
              variant="secondary"
              onClick={() => commands.pickLocalExecutable(targetReg.id)}
              data-testid="local-editor-locate-executable"
            >
              {t('drmFree.locateExecutable')}
            </Button>
          </div>

          {multipleExecutables && matchingCandidate ? (
            <div
              data-testid="local-editor-multiple-warning"
              className="flex items-center gap-compact text-xs text-warning"
            >
              <IconInfoCircle size={14} stroke={2} aria-hidden="true" />
              <span>{t('drmFree.severalPossibleExecutables')}</span>
            </div>
          ) : null}

          {multipleExecutables && matchingCandidate ? (
            <SelectField
              size="compact"
              className="max-w-[280px]"
              aria-label={t('drmFree.candidateExecutables')}
              data-testid="candidate-executable-select"
              value={executable}
              options={matchingCandidate.targets.map((target) => ({
                value: target,
                label: target,
              }))}
              onChange={handleCandidateChange}
            />
          ) : null}

          {coverError ? (
            <div data-testid="local-editor-cover-error">
              <Callout tone="danger" title={t(coverError)} />
            </div>
          ) : null}
        </div>
      </div>

      <div className="flex gap-sm">
        <Field label={t('drmFree.gameFolder')} className="min-w-0 flex-1">
          <TextInput
            data-testid="local-editor-root"
            aria-label={t('drmFree.gameFolder')}
            leadingIcon={IconFolder}
            value={root}
            onChange={(event) => setRoot(event.target.value)}
          />
        </Field>
        <Field label={t('drmFree.workingDirectory')} className="min-w-0 flex-1">
          <TextInput
            data-testid="local-editor-working-dir"
            aria-label={t('drmFree.workingDirectory')}
            leadingIcon={IconFolder}
            value={workingDirectory}
            onChange={(event) => setWorkingDirectory(event.target.value)}
          />
        </Field>
      </div>

      <Field label={t('drmFree.arguments')}>
        <TextInput
          data-testid="local-editor-arguments"
          aria-label={t('drmFree.arguments')}
          leadingIcon={IconTerminal2}
          value={argsText}
          onChange={(event) => setArgsText(event.target.value)}
          placeholder="-flag --param=value"
        />
      </Field>

      <div className="flex flex-col gap-sm">
        <button
          type="button"
          data-testid="local-editor-advanced"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
          className="flex items-center gap-compact self-start rounded-sm text-text-muted transition-colors hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
        >
          <IconChevronDown
            size={14}
            stroke={2}
            aria-hidden="true"
            className={advancedOpen ? 'text-text-dimmed' : '-rotate-90 text-text-dimmed'}
          />
          <span className="text-caption font-semibold uppercase tracking-label">
            {t('drmFree.advancedProcessDetection')}
          </span>
        </button>

        {advancedOpen ? (
          <div className="flex gap-sm">
            <Field
              label={t('drmFree.gameExecutablesOnePerLine')}
              className="min-w-0 flex-1"
            >
              <TextArea
                data-testid="local-editor-gameplay"
                aria-label={t('drmFree.gameExecutablesOnePerLine')}
                value={gameplayText}
                onChange={(event) => setGameplayText(event.target.value)}
              />
            </Field>
            <Field
              label={t('drmFree.launcherExecutablesOnePerLine')}
              className="min-w-0 flex-1"
            >
              <TextArea
                data-testid="local-editor-launchers"
                aria-label={t('drmFree.launcherExecutablesOnePerLine')}
                value={launchersText}
                onChange={(event) => setLaunchersText(event.target.value)}
              />
            </Field>
          </div>
        ) : null}
      </div>
    </DialogShell>
  );
};
