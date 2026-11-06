import React from 'react';
import { commands, useTools } from '../../api';
import { ErrorBoundary } from '../../diagnostics';
import { GameToolsModal } from './GameToolsModal';

/**
 * The game tools modal inside its own error boundary. A bad tools payload then shows a panel the
 * user can close, instead of blanking the app. The boundary clears when the open entry changes.
 * Closing the modal, or the panel's Close button, asks the engine to close it.
 */
export const GameToolsPanel: React.FC = () => {
  const tools = useTools();
  const modal = tools.modal;

  return (
    <ErrorBoundary
      scope="game-tools"
      variant="panel"
      resetKey={modal?.entryKey}
      context={{
        'Game entry': modal?.entryKey ?? 'none',
        'Active tool': modal?.activeToolId ?? 'none',
      }}
      onDismiss={() => {
        commands.closeTools();
      }}
    >
      <GameToolsModal />
    </ErrorBoundary>
  );
};
