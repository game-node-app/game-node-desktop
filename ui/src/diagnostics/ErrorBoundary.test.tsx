import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { ErrorBoundary } from './ErrorBoundary';
import { clearLogs, recordLog } from './logBuffer';

// Flip this to make the child throw during render.
let shouldThrow = false;

function Thrower() {
  if (shouldThrow) throw new Error('boom');
  return <p>child content</p>;
}

function reportBox(): HTMLTextAreaElement {
  return screen.getByRole('textbox', { name: 'Crash report' }) as HTMLTextAreaElement;
}

function setClipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
}

describe('ErrorBoundary', () => {
  let consoleError: { mockRestore(): void };

  beforeEach(() => {
    clearLogs();
    shouldThrow = false;
    // React logs every error it catches. Keep test output readable.
    consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  afterEach(() => {
    consoleError.mockRestore();
    delete (navigator as unknown as { clipboard?: unknown }).clipboard;
    delete (document as unknown as { execCommand?: unknown }).execCommand;
    document.documentElement.lang = '';
  });

  it('renders its children when nothing throws', () => {
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );
    expect(screen.getByText('child content')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('shows what happened and where, with a report that holds the error and recent logs', () => {
    recordLog('warn', ['loaded 3 games']);
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Error: boom', { selector: 'code' })).toBeInTheDocument();

    const report = reportBox().value;
    expect(report).toContain('Boundary:   app');
    expect(report).toContain('What happened: Error: boom');
    expect(report).toMatch(/Where:\s+Thrower/);
    expect(report).toContain('[warn] loaded 3 games');
    expect(report).toContain('== Error ==');
  });

  it('copies the full report and confirms it', async () => {
    const writeText = vi.fn((_text: string) => Promise.resolve());
    setClipboard(writeText);
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Copy report and logs' }));

    expect(await screen.findByText('Copied to the clipboard.')).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText.mock.calls[0]?.[0]).toContain('What happened: Error: boom');
  });

  it('says so when copying fails, and keeps the report selectable', async () => {
    setClipboard(() => Promise.reject(new Error('denied')));
    Object.defineProperty(document, 'execCommand', { value: () => false, configurable: true });
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Copy report and logs' }));

    expect(await screen.findByText(/Could not copy automatically/)).toBeInTheDocument();
    expect(reportBox()).toHaveAttribute('readonly');
  });

  it('Try again renders the children once the cause is gone', () => {
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );

    shouldThrow = false;
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));

    expect(screen.getByText('child content')).toBeInTheDocument();
  });

  it('the page variant offers a reload and no close', () => {
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );

    expect(screen.getByRole('button', { name: 'Reload app' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Close' })).not.toBeInTheDocument();
  });

  it('the panel variant is a dialog whose Close and Escape call onDismiss', () => {
    const onDismiss = vi.fn();
    shouldThrow = true;
    render(
      <ErrorBoundary scope="game-tools" variant="panel" onDismiss={onDismiss}>
        <Thrower />
      </ErrorBoundary>,
    );

    expect(screen.getByRole('alertdialog')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Reload app' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });

  it('a change of resetKey after a crash renders the children again', () => {
    shouldThrow = true;
    const { rerender } = render(
      <ErrorBoundary scope="game-tools" variant="panel" resetKey="steam:1" onDismiss={() => {}}>
        <Thrower />
      </ErrorBoundary>,
    );
    expect(screen.getByRole('alertdialog')).toBeInTheDocument();

    shouldThrow = false;
    rerender(
      <ErrorBoundary scope="game-tools" variant="panel" resetKey="steam:2" onDismiss={() => {}}>
        <Thrower />
      </ErrorBoundary>,
    );
    expect(screen.getByText('child content')).toBeInTheDocument();
  });

  it('includes the caller context in the report', () => {
    shouldThrow = true;
    render(
      <ErrorBoundary
        scope="game-tools"
        variant="panel"
        context={{ 'Game entry': 'steam:1' }}
        onDismiss={() => {}}
      >
        <Thrower />
      </ErrorBoundary>,
    );
    expect(reportBox().value).toContain('Game entry: steam:1');
  });

  it('uses Portuguese when the document language is pt-BR', () => {
    document.documentElement.lang = 'pt-BR';
    shouldThrow = true;
    render(
      <ErrorBoundary scope="app" variant="page">
        <Thrower />
      </ErrorBoundary>,
    );
    expect(screen.getByText('Algo deu errado')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Tentar novamente' })).toBeInTheDocument();
  });
});
