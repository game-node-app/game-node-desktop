// Recent frontend diagnostics kept in memory, so a crash report can include what happened
// just before the failure. Nothing here is written to disk or sent anywhere; the buffer is
// read only when a report is built.

export type LogLevel = 'error' | 'warn' | 'uncaught' | 'rejection';

export interface LogEntry {
  /** ISO-8601 timestamp of when the entry was recorded. */
  time: string;
  level: LogLevel;
  message: string;
}

export const MAX_LOG_ENTRIES = 150;
export const MAX_MESSAGE_CHARS = 2000;

let entries: LogEntry[] = [];

/** Turns any logged value into text. Errors keep their stack; objects become JSON when possible. */
export function formatLogValue(value: unknown): string {
  if (value instanceof Error) {
    const header = `${value.name}: ${value.message}`;
    if (value.stack) return value.stack.includes(value.message) ? value.stack : `${header}\n${value.stack}`;
    return header;
  }
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return safeString(value);
  }
}

function safeString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return '[unprintable value]';
  }
}

function truncate(text: string): string {
  if (text.length <= MAX_MESSAGE_CHARS) return text;
  const dropped = text.length - MAX_MESSAGE_CHARS;
  return `${text.slice(0, MAX_MESSAGE_CHARS)}… [truncated ${dropped} characters]`;
}

export function recordLog(level: LogLevel, values: readonly unknown[]): void {
  const message = truncate(values.map(formatLogValue).join(' '));
  entries.push({ time: new Date().toISOString(), level, message });
  if (entries.length > MAX_LOG_ENTRIES) {
    entries = entries.slice(entries.length - MAX_LOG_ENTRIES);
  }
}

/** Oldest first. Returns a copy, so callers cannot change the buffer. */
export function getRecentLogs(): LogEntry[] {
  return entries.slice();
}

export function clearLogs(): void {
  entries = [];
}

let activeUninstall: (() => void) | null = null;

/**
 * Mirrors `console.error`, `console.warn`, uncaught window errors and unhandled promise
 * rejections into the buffer. The original console methods still run, so devtools output is
 * unchanged. Calling it again returns the existing uninstall function.
 */
export function installLogCapture(): () => void {
  if (activeUninstall) return activeUninstall;

  const originalError = console.error;
  const originalWarn = console.warn;

  console.error = (...args: unknown[]) => {
    recordLog('error', args);
    originalError.apply(console, args);
  };
  console.warn = (...args: unknown[]) => {
    recordLog('warn', args);
    originalWarn.apply(console, args);
  };

  const onError = (event: ErrorEvent) => {
    const parts: unknown[] = [event.error ?? event.message];
    if (event.filename) parts.push(`(${event.filename}:${event.lineno}:${event.colno})`);
    recordLog('uncaught', parts);
  };
  const onRejection = (event: Event) => {
    const reason = (event as PromiseRejectionEvent).reason;
    recordLog('rejection', [reason ?? 'unhandled promise rejection']);
  };
  window.addEventListener('error', onError);
  window.addEventListener('unhandledrejection', onRejection);

  activeUninstall = () => {
    console.error = originalError;
    console.warn = originalWarn;
    window.removeEventListener('error', onError);
    window.removeEventListener('unhandledrejection', onRejection);
    activeUninstall = null;
  };
  return activeUninstall;
}
