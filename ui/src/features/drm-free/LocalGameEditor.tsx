import React, { useState, useEffect } from 'react';
import { IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
import { commands, useLocalGames, type Registration } from '../../api';
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

const fieldLabelClass = 'mb-1.5 block text-sm font-medium text-text-default';
const fieldInputClass =
  'w-full rounded-sm border border-border-subtle bg-secondary px-sm py-compact text-sm text-text-default placeholder:text-text-dimmed focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand';
const secondaryButtonClass =
  'inline-flex items-center justify-center whitespace-nowrap rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand';

export const LocalGameEditor: React.FC<LocalGameEditorProps> = ({ registration: propRegistration, onClose }) => {
  const { t } = useI18n();
  const localGames = useLocalGames();
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

  const [title, setTitle] = useState('');
  const [executable, setExecutable] = useState('');
  const [root, setRoot] = useState('');
  const [workingDirectory, setWorkingDirectory] = useState('');
  const [argsText, setArgsText] = useState('');
  const [gameplayText, setGameplayText] = useState('');
  const [launchersText, setLaunchersText] = useState('');

  useEffect(() => {
    if (targetReg) {
      setTitle(targetReg.title);
      setExecutable(targetReg.executable);
      setRoot(targetReg.root);
      setWorkingDirectory(targetReg.workingDirectory);
      setArgsText(targetReg.arguments.join(' '));
      setGameplayText(targetReg.gameplay.join('\n'));
      setLaunchersText(targetReg.launchers.join('\n'));
    }
  }, [targetReg?.id, targetReg?.revision]);

  const handleClose = () => {
    uiStore.setState((s) => ({ ...s, localEditorEntryKey: null }));
    onClose?.();
  };

  if (!isOpen || !targetReg) {
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

  const handleSave = () => {
    const updated: Registration = {
      ...targetReg,
      title: title.trim(),
      executable: executable.trim(),
      root: root.trim(),
      workingDirectory: workingDirectory.trim(),
      arguments: parseArguments(argsText),
      gameplay: gameplayText
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean),
      launchers: launchersText
        .split('\n')
        .map((s) => s.trim())
        .filter(Boolean),
    };

    commands.editLocalGame(updated);
    handleClose();
  };

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="local-game-editor"
      defaultSize={{ width: 640, height: 560 }}
      minSize={{ width: 520, height: 400 }}
      zIndex={1000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.62} />
      <FloatingModal.Content className="flex flex-col bg-chrome">
        {/* Header */}
        <FloatingModal.Header className="bg-chrome px-control-inline py-md">
          <div>
            <FloatingModal.Title className="text-lg font-semibold text-text-default">
              {t('drmFree.gameSettings')}
            </FloatingModal.Title>
            <div className="text-xs text-text-dimmed">{targetReg.title}</div>
          </div>
          <FloatingModal.CloseButton
            aria-label="Close"
            icon={<IconX size={18} stroke={1.5} />}
            className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
          />
        </FloatingModal.Header>

        {/* Content */}
        <FloatingModal.Body className="gap-md p-control-inline">
          {/* Title */}
          <div>
            <label htmlFor="local-editor-title" className={fieldLabelClass}>
              {t('drmFree.title')}
            </label>
            <input
              id="local-editor-title"
              data-testid="local-editor-title"
              aria-label="Title"
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              className={fieldInputClass}
            />
          </div>

          {/* Executable */}
          <div>
            <label htmlFor="local-editor-executable" className={fieldLabelClass}>
              {t('drmFree.executable')}
            </label>
            <div className="flex gap-compact">
              <input
                id="local-editor-executable"
                data-testid="local-editor-executable"
                aria-label="Executable"
                type="text"
                value={executable}
                onChange={(e) => setExecutable(e.target.value)}
                className={`${fieldInputClass} flex-1`}
              />
              <button
                type="button"
                onClick={() => commands.pickLocalExecutable(targetReg.id)}
                className={secondaryButtonClass}
              >
                {t('drmFree.locateExecutable')}
              </button>
            </div>
          </div>

          {/* Candidate executable switcher */}
          {matchingCandidate && matchingCandidate.targets.length > 1 && (
            <div className="flex flex-col gap-compact rounded-sm border border-border-subtle bg-secondary p-sm">
              <div className="text-xs text-text-dimmed">
                {t('drmFree.severalLaunchTargetsChooseAnExecutableBelow')}
              </div>
              <select
                data-testid="candidate-executable-select"
                aria-label="Candidate executables"
                value={executable}
                onChange={(e) => handleCandidateChange(e.target.value)}
                className="w-full cursor-pointer rounded-sm border border-border-subtle bg-raised px-2.5 py-1.5 text-sm text-text-default focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand"
              >
                {matchingCandidate.targets.map((tgt) => (
                  <option key={tgt} value={tgt}>
                    {tgt}
                  </option>
                ))}
              </select>
              <div className="flex flex-wrap gap-1.5">
                {matchingCandidate.targets.map((tgt) => (
                  <button
                    key={tgt}
                    type="button"
                    onClick={() => handleCandidateChange(tgt)}
                    className={`rounded-sm border border-border-subtle px-2.5 py-1 text-xs transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${
                      executable === tgt
                        ? 'bg-brand text-on-primary'
                        : 'bg-raised text-text-default hover:bg-surface'
                    }`}
                  >
                    {tgt}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Root Folder */}
          <div>
            <label htmlFor="local-editor-root" className={fieldLabelClass}>
              {t('drmFree.gameFolder')}
            </label>
            <input
              id="local-editor-root"
              data-testid="local-editor-root"
              aria-label="Game folder"
              type="text"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
              className={fieldInputClass}
            />
          </div>

          {/* Working Directory */}
          <div>
            <label htmlFor="local-editor-working-dir" className={fieldLabelClass}>
              {t('drmFree.workingDirectory')}
            </label>
            <input
              id="local-editor-working-dir"
              data-testid="local-editor-working-dir"
              aria-label="Working directory"
              type="text"
              value={workingDirectory}
              onChange={(e) => setWorkingDirectory(e.target.value)}
              className={fieldInputClass}
            />
          </div>

          {/* Arguments */}
          <div>
            <label htmlFor="local-editor-arguments" className={fieldLabelClass}>
              {t('drmFree.arguments')}
            </label>
            <input
              id="local-editor-arguments"
              data-testid="local-editor-arguments"
              aria-label="Arguments"
              type="text"
              value={argsText}
              onChange={(e) => setArgsText(e.target.value)}
              placeholder="-flag --param=value"
              className={fieldInputClass}
            />
          </div>

          {/* Gameplay Executables */}
          <div>
            <label htmlFor="local-editor-gameplay" className={fieldLabelClass}>
              {t('drmFree.gameplayExecutablesOnePerLine')}
            </label>
            <textarea
              id="local-editor-gameplay"
              data-testid="local-editor-gameplay"
              aria-label="Gameplay executables (one per line)"
              rows={3}
              value={gameplayText}
              onChange={(e) => setGameplayText(e.target.value)}
              className={`${fieldInputClass} resize-y`}
            />
          </div>

          {/* Launcher Executables */}
          <div>
            <label htmlFor="local-editor-launchers" className={fieldLabelClass}>
              {t('drmFree.launcherExecutablesOnePerLine')}
            </label>
            <textarea
              id="local-editor-launchers"
              data-testid="local-editor-launchers"
              aria-label="Launcher executables (one per line)"
              rows={3}
              value={launchersText}
              onChange={(e) => setLaunchersText(e.target.value)}
              className={`${fieldInputClass} resize-y`}
            />
          </div>
        </FloatingModal.Body>

        {/* Footer */}
        <FloatingModal.Footer className="px-control-inline py-md">
          <button type="button" onClick={handleClose} className={secondaryButtonClass}>
            {t('drmFree.cancel')}
          </button>
          <button
            type="button"
            onClick={handleSave}
            className="inline-flex items-center justify-center rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.save')}
          </button>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
