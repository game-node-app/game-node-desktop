import { formatLogValue, type LogEntry } from './logBuffer';

export interface CrashDetails {
  /** Technical id of the boundary that caught the error, e.g. "app" or "game-tools". */
  scope: string;
  error: unknown;
  /** React's component stack for the failing render, when React reports one. */
  componentStack: string | null;
  capturedAt: string;
  logs: readonly LogEntry[];
  /** Extra facts from the boundary's owner, keyed by a readable label. */
  context: Readonly<Record<string, string>>;
}

export interface ReportEnvironment {
  userAgent: string;
  language: string;
}

export interface ErrorSummary {
  /** "TypeError: Cannot read properties of undefined", or the logged text for non-Error values. */
  headline: string;
  stack: string | null;
}

export function describeError(error: unknown): ErrorSummary {
  if (error instanceof Error) {
    return { headline: `${error.name}: ${error.message}`, stack: error.stack ?? null };
  }
  return { headline: formatLogValue(error), stack: null };
}

/** The innermost component in a React component stack, e.g. "GameToolsModal". */
export function failingComponent(componentStack: string | null): string | null {
  const match = componentStack?.match(/^\s*at (\S+)/m);
  return match ? match[1] : null;
}

export function readEnvironment(): ReportEnvironment {
  const hasNavigator = typeof navigator !== 'undefined';
  return {
    userAgent: hasNavigator ? navigator.userAgent : 'unknown',
    language: hasNavigator ? navigator.language : 'unknown',
  };
}

function formatEntry(entry: LogEntry): string {
  return `${entry.time} [${entry.level}] ${entry.message}`;
}

/** Plain text for the clipboard and for bug reports. Sections are fixed so they are easy to scan. */
export function buildCrashReport(
  details: CrashDetails,
  env: ReportEnvironment = readEnvironment(),
): string {
  const error = describeError(details.error);
  const where = failingComponent(details.componentStack);
  const logLines =
    details.logs.length === 0
      ? ['(no entries)']
      : details.logs.map(formatEntry);

  const lines = [
    'GameNode Desktop crash report',
    `Captured:   ${details.capturedAt}`,
    `Boundary:   ${details.scope}`,
    ...Object.entries(details.context).map(([label, value]) => `${label}: ${value}`),
    `Language:   ${env.language}`,
    `User agent: ${env.userAgent}`,
    '',
    `What happened: ${error.headline}`,
    ...(where ? [`Where:         ${where}`] : []),
    '',
    '== Error ==',
    error.stack ?? error.headline,
    '',
    '== Component stack ==',
    details.componentStack?.trim() || 'Not reported by React.',
    '',
    `== Recent log (${details.logs.length} ${details.logs.length === 1 ? 'entry' : 'entries'}, oldest first) ==`,
    ...logLines,
  ];
  return lines.join('\n');
}
