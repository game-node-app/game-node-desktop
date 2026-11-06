import React, { useEffect, useId } from 'react';
import { copyText } from './crashCopy';
import { buildCrashReport, describeError, failingComponent, type CrashDetails } from './crashReport';
import { resolveCrashStrings } from './crashStrings';
import { getRecentLogs, type LogEntry } from './logBuffer';

export type BoundaryVariant = 'page' | 'panel';

export interface ErrorBoundaryProps {
  /** Technical name for the boundary, shown in the report, e.g. "app" or "game-tools". */
  scope: string;
  /**
   * `page` replaces a whole screen and offers a reload. `panel` replaces one overlay, which the
   * user can close.
   */
  variant: BoundaryVariant;
  /**
   * After a crash, a change to this value clears the error and renders the children again. Pass
   * the identity of what the children show, such as the open entry.
   */
  resetKey?: unknown;
  /** Panel only: the user asked to close the panel with its Close button or Escape. */
  onDismiss?: () => void;
  /** Extra facts for the report, read when the fallback renders. */
  context?: Record<string, string>;
  children?: React.ReactNode;
}

type CopyStatus = 'idle' | 'copied' | 'failed';

interface CrashRecord {
  error: unknown;
  capturedAt: string;
  componentStack: string | null;
  logs: LogEntry[];
}

interface ErrorBoundaryState {
  crash: CrashRecord | null;
  copyStatus: CopyStatus;
}

/**
 * Catches render errors below it and shows what happened: the error, the component it came
 * from, the time, and a report with recent logs that can be copied.
 *
 * Crash handling is kept in this class so it works when hooks in the failing tree are what
 * broke. The fallback is a function component, so it can use hooks.
 */
export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { crash: null, copyStatus: 'idle' };

  static getDerivedStateFromError(error: unknown): Partial<ErrorBoundaryState> {
    return {
      crash: {
        error,
        capturedAt: new Date().toISOString(),
        componentStack: null,
        logs: getRecentLogs(),
      },
      copyStatus: 'idle',
    };
  }

  componentDidCatch(error: unknown, info: React.ErrorInfo): void {
    // React gives the component stack only here. Re-read the logs too, so the report includes
    // React's own error line.
    this.setState((state) => ({
      crash: {
        error,
        capturedAt: state.crash?.capturedAt ?? new Date().toISOString(),
        componentStack: info.componentStack ?? null,
        logs: getRecentLogs(),
      },
    }));
  }

  componentDidUpdate(prevProps: ErrorBoundaryProps, prevState: ErrorBoundaryState): void {
    // Clear only when a crash was already showing before this update. A crash raised in the
    // same update that changed the key must stay, or the boundary would clear and retry in a loop.
    if (prevState.crash && !Object.is(prevProps.resetKey, this.props.resetKey)) {
      this.setState({ crash: null, copyStatus: 'idle' });
    }
  }

  private handleRetry = (): void => {
    this.setState({ crash: null, copyStatus: 'idle' });
  };

  private handleCopy = async (report: string): Promise<void> => {
    const ok = await copyText(report);
    this.setState({ copyStatus: ok ? 'copied' : 'failed' });
  };

  render(): React.ReactNode {
    const { crash, copyStatus } = this.state;
    if (!crash) return this.props.children ?? null;

    const details: CrashDetails = {
      scope: this.props.scope,
      error: crash.error,
      componentStack: crash.componentStack,
      capturedAt: crash.capturedAt,
      logs: crash.logs,
      context: this.props.context ?? {},
    };
    return (
      <CrashFallback
        details={details}
        variant={this.props.variant}
        copyStatus={copyStatus}
        onCopy={this.handleCopy}
        onRetry={this.handleRetry}
        onDismiss={this.props.onDismiss}
      />
    );
  }
}

interface CrashFallbackProps {
  details: CrashDetails;
  variant: BoundaryVariant;
  copyStatus: CopyStatus;
  onCopy: (report: string) => void;
  onRetry: () => void;
  onDismiss?: () => void;
}

function documentLang(): string {
  return typeof document === 'undefined' ? '' : document.documentElement.lang;
}

function formatTime(iso: string, lang: string): string {
  try {
    return new Date(iso).toLocaleString(lang || undefined);
  } catch {
    return iso;
  }
}

function CrashFallback({
  details,
  variant,
  copyStatus,
  onCopy,
  onRetry,
  onDismiss,
}: CrashFallbackProps) {
  const headingId = useId();
  const lang = documentLang();
  const strings = resolveCrashStrings(lang);
  const isPanel = variant === 'panel';
  const error = describeError(details.error);
  const where = failingComponent(details.componentStack);
  const report = buildCrashReport(details);

  // Escape closes a panel the way it closes the modal. The page variant has nothing to close.
  useEffect(() => {
    if (!isPanel || !onDismiss) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onDismiss();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isPanel, onDismiss]);

  const statusText =
    copyStatus === 'copied' ? strings.copied : copyStatus === 'failed' ? strings.copyFailed : '';

  const content = (
    <>
      <header style={headerStyle}>
        <h2 id={headingId} style={titleStyle}>
          {strings.title}
        </h2>
        <p style={introStyle}>{isPanel ? strings.panelIntro : strings.pageIntro}</p>
      </header>

      <div style={bodyStyle}>
        <dl style={factsStyle}>
          <Fact label={strings.whatHappened}>
            <code style={codeStyle}>{error.headline}</code>
          </Fact>
          <Fact label={strings.where}>{where ?? strings.notReported}</Fact>
          <Fact label={strings.area}>
            <code style={codeStyle}>{details.scope}</code>
          </Fact>
          <Fact label={strings.when}>{formatTime(details.capturedAt, lang)}</Fact>
        </dl>

        {error.stack && (
          <details style={detailsStyle}>
            <summary style={summaryStyle}>{strings.errorStack}</summary>
            <pre style={preStyle}>{error.stack}</pre>
          </details>
        )}

        {details.componentStack?.trim() && (
          <details style={detailsStyle}>
            <summary style={summaryStyle}>{strings.componentStack}</summary>
            <pre style={preStyle}>{details.componentStack.trim()}</pre>
          </details>
        )}

        <div style={reportBlockStyle}>
          <div style={reportHeaderStyle}>
            <span style={labelStyle}>{strings.reportLabel}</span>
            <button type="button" onClick={() => onCopy(report)} style={secondaryButtonStyle}>
              {strings.copy}
            </button>
          </div>
          <textarea
            readOnly
            aria-label={strings.reportLabel}
            value={report}
            rows={10}
            onFocus={(event) => event.currentTarget.select()}
            style={textareaStyle}
          />
          <p style={hintStyle}>
            {strings.logCount(details.logs.length)} {strings.privacyNote}
          </p>
          <p role="status" aria-live="polite" style={hintStyle}>
            {statusText}
          </p>
        </div>
      </div>

      <footer style={footerStyle}>
        {isPanel
          ? onDismiss && (
              <button type="button" onClick={onDismiss} style={secondaryButtonStyle}>
                {strings.close}
              </button>
            )
          : (
              <button
                type="button"
                onClick={() => window.location.reload()}
                style={secondaryButtonStyle}
              >
                {strings.reload}
              </button>
            )}
        <button type="button" onClick={onRetry} style={primaryButtonStyle}>
          {strings.retry}
        </button>
      </footer>
    </>
  );

  if (isPanel) {
    return (
      <div style={panelOverlayStyle}>
        <section
          role="alertdialog"
          aria-modal="true"
          aria-labelledby={headingId}
          style={panelCardStyle}
        >
          {content}
        </section>
      </div>
    );
  }

  return (
    <div style={pageStyle}>
      <section role="alert" aria-labelledby={headingId} style={pageCardStyle}>
        {content}
      </section>
    </div>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
      <dt style={labelStyle}>{label}</dt>
      <dd style={{ margin: 0, fontSize: 'var(--font-size-sm, 14px)', wordBreak: 'break-word' }}>
        {children}
      </dd>
    </div>
  );
}

const monoFont = 'ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace';

const panelOverlayStyle: React.CSSProperties = {
  position: 'fixed',
  inset: 0,
  backgroundColor: 'var(--settings-overlay, rgba(0, 0, 0, 0.62))',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 1000,
  padding: 'var(--modal-margin, 24px)',
};

const panelCardStyle: React.CSSProperties = {
  width: '100%',
  maxWidth: '720px',
  maxHeight: '90vh',
  display: 'flex',
  flexDirection: 'column',
  overflow: 'hidden',
  backgroundColor: 'var(--settings-bg-base, #111111)',
  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
  borderRadius: 'var(--radius-md, 8px)',
  boxShadow: 'var(--shadow-modal)',
  color: 'var(--color-text-default, #D9D9D9)',
};

const pageStyle: React.CSSProperties = {
  width: '100%',
  height: '100vh',
  overflow: 'auto',
  display: 'flex',
  alignItems: 'flex-start',
  justifyContent: 'center',
  padding: '24px',
  boxSizing: 'border-box',
  backgroundColor: 'var(--color-background, #161616)',
  color: 'var(--color-text-default, #D9D9D9)',
};

const pageCardStyle: React.CSSProperties = {
  width: '100%',
  maxWidth: '720px',
  display: 'flex',
  flexDirection: 'column',
  backgroundColor: 'var(--settings-bg-base, #111111)',
  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
  borderRadius: 'var(--radius-md, 8px)',
  boxShadow: 'var(--shadow-modal)',
};

const headerStyle: React.CSSProperties = {
  padding: '16px 24px',
  borderBottom: '1px solid var(--settings-border-subtle, #2A2A2A)',
};

const titleStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 'var(--font-size-lg, 18px)',
  fontWeight: 600,
};

const introStyle: React.CSSProperties = {
  margin: '6px 0 0',
  fontSize: 'var(--font-size-sm, 14px)',
  color: 'var(--color-text-dimmed, #808080)',
};

const bodyStyle: React.CSSProperties = {
  padding: '20px 24px',
  overflowY: 'auto',
  minHeight: 0,
  display: 'flex',
  flexDirection: 'column',
  gap: '16px',
};

const factsStyle: React.CSSProperties = {
  margin: 0,
  display: 'grid',
  gap: '10px',
};

const labelStyle: React.CSSProperties = {
  fontSize: 'var(--font-size-xs, 12px)',
  fontWeight: 600,
  color: 'var(--color-text-dimmed, #808080)',
};

const codeStyle: React.CSSProperties = {
  fontFamily: monoFont,
  fontSize: 'var(--font-size-xs, 12px)',
  color: 'var(--color-text-default, #D9D9D9)',
  wordBreak: 'break-word',
};

const detailsStyle: React.CSSProperties = {
  fontSize: 'var(--font-size-sm, 14px)',
};

const summaryStyle: React.CSSProperties = {
  cursor: 'pointer',
  color: 'var(--color-text-dimmed, #808080)',
  fontSize: 'var(--font-size-xs, 12px)',
  fontWeight: 600,
  marginBottom: '6px',
};

const preStyle: React.CSSProperties = {
  margin: 0,
  padding: '10px 12px',
  maxHeight: '220px',
  overflow: 'auto',
  backgroundColor: 'var(--settings-bg-base, #0B0B0B)',
  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
  borderRadius: 'var(--radius-sm, 4px)',
  fontFamily: monoFont,
  fontSize: 'var(--font-size-xs, 12px)',
  whiteSpace: 'pre-wrap',
  wordBreak: 'break-word',
};

const reportBlockStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: '6px',
};

const reportHeaderStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  gap: '8px',
};

const textareaStyle: React.CSSProperties = {
  width: '100%',
  minHeight: '180px',
  boxSizing: 'border-box',
  backgroundColor: 'var(--settings-bg-base, #0B0B0B)',
  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
  borderRadius: 'var(--radius-sm, 4px)',
  padding: '10px 12px',
  color: 'var(--color-text-default, #D9D9D9)',
  fontFamily: monoFont,
  fontSize: 'var(--font-size-xs, 12px)',
  resize: 'vertical',
};

const hintStyle: React.CSSProperties = {
  margin: 0,
  fontSize: 'var(--font-size-xs, 12px)',
  color: 'var(--color-text-dimmed, #808080)',
};

const footerStyle: React.CSSProperties = {
  padding: '16px 24px',
  borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
  display: 'flex',
  justifyContent: 'flex-end',
  flexWrap: 'wrap',
  gap: '8px',
};

const secondaryButtonStyle: React.CSSProperties = {
  padding: '8px 16px',
  backgroundColor: 'var(--settings-bg-raised, #222222)',
  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
  borderRadius: 'var(--radius-sm, 4px)',
  color: 'var(--color-text-default, #D9D9D9)',
  fontSize: 'var(--font-size-sm, 14px)',
  cursor: 'pointer',
};

const primaryButtonStyle: React.CSSProperties = {
  padding: '8px 20px',
  backgroundColor: 'var(--color-primary, #F15025)',
  border: 'none',
  borderRadius: 'var(--radius-sm, 4px)',
  color: 'var(--color-on-primary, #EDEDED)',
  fontSize: 'var(--font-size-sm, 14px)',
  fontWeight: 600,
  cursor: 'pointer',
};
