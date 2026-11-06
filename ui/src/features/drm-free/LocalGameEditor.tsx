import React, { useState, useEffect } from 'react';
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

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen]);

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
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="local-editor-heading"
      style={{
        position: 'fixed',
        inset: 0,
        backgroundColor: 'var(--settings-overlay, rgba(0, 0, 0, 0.62))',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        padding: 'var(--modal-margin, 24px)',
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) handleClose();
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '640px',
          maxHeight: '90vh',
          backgroundColor: 'var(--settings-bg-base, #111111)',
          border: '1px solid var(--settings-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          boxShadow: 'var(--shadow-modal)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: '16px 24px',
            borderBottom: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div>
            <h2 id="local-editor-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('drmFree.gameSettings')}
            </h2>
            <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
              {targetReg.title}
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={handleClose}
            style={{
              padding: '6px 12px',
              backgroundColor: 'transparent',
              color: 'var(--color-text-dimmed, #808080)',
              borderRadius: 'var(--radius-sm, 4px)',
              cursor: 'pointer',
            }}
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div
          style={{
            padding: '24px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '16px',
          }}
        >
          {/* Title */}
          <div>
            <label
              htmlFor="local-editor-title"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.title')}
            </label>
            <input
              id="local-editor-title"
              data-testid="local-editor-title"
              aria-label="Title"
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            />
          </div>

          {/* Executable */}
          <div>
            <label
              htmlFor="local-editor-executable"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.executable')}
            </label>
            <div style={{ display: 'flex', gap: '8px' }}>
              <input
                id="local-editor-executable"
                data-testid="local-editor-executable"
                aria-label="Executable"
                type="text"
                value={executable}
                onChange={(e) => setExecutable(e.target.value)}
                style={{
                  flex: 1,
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '8px 12px',
                  color: 'var(--color-text-default, #D9D9D9)',
                }}
              />
              <button
                type="button"
                onClick={() => commands.pickLocalExecutable(targetReg.id)}
                style={{
                  backgroundColor: 'var(--settings-bg-raised, #222222)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '8px 16px',
                  color: 'var(--color-text-default, #D9D9D9)',
                  cursor: 'pointer',
                  whiteSpace: 'nowrap',
                }}
              >
                {t('drmFree.locateExecutable')}
              </button>
            </div>
          </div>

          {/* Candidate executable switcher */}
          {matchingCandidate && matchingCandidate.targets.length > 1 && (
            <div
              style={{
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '12px',
                display: 'flex',
                flexDirection: 'column',
                gap: '8px',
              }}
            >
              <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
                {t('drmFree.severalLaunchTargetsChooseAnExecutableBelow')}
              </div>
              <select
                data-testid="candidate-executable-select"
                aria-label="Candidate executables"
                value={executable}
                onChange={(e) => handleCandidateChange(e.target.value)}
                style={{
                  width: '100%',
                  backgroundColor: 'var(--settings-bg-raised, #222222)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '6px 10px',
                  color: 'var(--color-text-default, #D9D9D9)',
                }}
              >
                {matchingCandidate.targets.map((tgt) => (
                  <option key={tgt} value={tgt}>
                    {tgt}
                  </option>
                ))}
              </select>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
                {matchingCandidate.targets.map((tgt) => (
                  <button
                    key={tgt}
                    type="button"
                    onClick={() => handleCandidateChange(tgt)}
                    style={{
                      padding: '4px 10px',
                      borderRadius: 'var(--radius-sm, 4px)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      backgroundColor: executable === tgt ? 'var(--color-primary, #F15025)' : 'var(--settings-bg-raised, #222222)',
                      color: executable === tgt ? 'var(--color-on-primary, #EDEDED)' : 'var(--color-text-default, #D9D9D9)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      cursor: 'pointer',
                    }}
                  >
                    {tgt}
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Root Folder */}
          <div>
            <label
              htmlFor="local-editor-root"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.gameFolder')}
            </label>
            <input
              id="local-editor-root"
              data-testid="local-editor-root"
              aria-label="Game folder"
              type="text"
              value={root}
              onChange={(e) => setRoot(e.target.value)}
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            />
          </div>

          {/* Working Directory */}
          <div>
            <label
              htmlFor="local-editor-working-dir"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.workingDirectory')}
            </label>
            <input
              id="local-editor-working-dir"
              data-testid="local-editor-working-dir"
              aria-label="Working directory"
              type="text"
              value={workingDirectory}
              onChange={(e) => setWorkingDirectory(e.target.value)}
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            />
          </div>

          {/* Arguments */}
          <div>
            <label
              htmlFor="local-editor-arguments"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
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
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            />
          </div>

          {/* Gameplay Executables */}
          <div>
            <label
              htmlFor="local-editor-gameplay"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.gameplayExecutablesOnePerLine')}
            </label>
            <textarea
              id="local-editor-gameplay"
              data-testid="local-editor-gameplay"
              aria-label="Gameplay executables (one per line)"
              rows={3}
              value={gameplayText}
              onChange={(e) => setGameplayText(e.target.value)}
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
                resize: 'vertical',
              }}
            />
          </div>

          {/* Launcher Executables */}
          <div>
            <label
              htmlFor="local-editor-launchers"
              style={{ display: 'block', marginBottom: '6px', fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}
            >
              {t('drmFree.launcherExecutablesOnePerLine')}
            </label>
            <textarea
              id="local-editor-launchers"
              data-testid="local-editor-launchers"
              aria-label="Launcher executables (one per line)"
              rows={3}
              value={launchersText}
              onChange={(e) => setLaunchersText(e.target.value)}
              style={{
                width: '100%',
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-sm, 4px)',
                padding: '8px 12px',
                color: 'var(--color-text-default, #D9D9D9)',
                resize: 'vertical',
              }}
            />
          </div>
        </div>

        {/* Footer */}
        <div
          style={{
            padding: '16px 24px',
            borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            justifyContent: 'flex-end',
            gap: '8px',
          }}
        >
          <button
            type="button"
            onClick={handleClose}
            style={{
              padding: '8px 16px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
            }}
          >
            {t('drmFree.cancel')}
          </button>
          <button
            type="button"
            onClick={handleSave}
            style={{
              padding: '8px 20px',
              backgroundColor: 'var(--color-primary, #F15025)',
              border: 'none',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-on-primary, #EDEDED)',
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            {t('drmFree.save')}
          </button>
        </div>
      </div>
    </div>
  );
};
