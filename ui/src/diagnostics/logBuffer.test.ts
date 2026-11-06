import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearLogs,
  formatLogValue,
  getRecentLogs,
  installLogCapture,
  MAX_LOG_ENTRIES,
  MAX_MESSAGE_CHARS,
  recordLog,
} from './logBuffer';

describe('formatLogValue', () => {
  it('keeps the error name, message and stack for Error values', () => {
    expect(formatLogValue(new TypeError('nope'))).toContain('TypeError: nope');
  });

  it('passes strings through and serializes plain objects', () => {
    expect(formatLogValue('plain')).toBe('plain');
    expect(formatLogValue({ a: 1 })).toBe('{"a":1}');
  });

  it('does not throw on circular objects', () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    expect(formatLogValue(circular)).toBe('[object Object]');
  });

  it('renders undefined', () => {
    expect(formatLogValue(undefined)).toBe('undefined');
  });
});

describe('log buffer', () => {
  beforeEach(() => clearLogs());

  it('records level, a timestamp and the joined message, oldest first', () => {
    recordLog('error', ['failed to load', 42]);
    recordLog('warn', ['slow']);
    const logs = getRecentLogs();
    expect(logs.map((entry) => [entry.level, entry.message])).toEqual([
      ['error', 'failed to load 42'],
      ['warn', 'slow'],
    ]);
    expect(Number.isNaN(Date.parse(logs[0].time))).toBe(false);
  });

  it('truncates very long messages and says how much was cut', () => {
    recordLog('error', ['x'.repeat(MAX_MESSAGE_CHARS + 50)]);
    const [entry] = getRecentLogs();
    expect(entry.message.startsWith('x'.repeat(MAX_MESSAGE_CHARS))).toBe(true);
    expect(entry.message).toContain('[truncated 50 characters]');
  });

  it('keeps only the most recent entries', () => {
    for (let i = 0; i < MAX_LOG_ENTRIES + 5; i += 1) recordLog('warn', [`entry ${i}`]);
    const logs = getRecentLogs();
    expect(logs).toHaveLength(MAX_LOG_ENTRIES);
    expect(logs[0].message).toBe('entry 5');
    expect(logs.at(-1)?.message).toBe(`entry ${MAX_LOG_ENTRIES + 4}`);
  });

  it('returns a copy, so callers cannot change the buffer', () => {
    recordLog('warn', ['one']);
    getRecentLogs().length = 0;
    expect(getRecentLogs()).toHaveLength(1);
  });
});

describe('installLogCapture', () => {
  let errorSpy: { mock: { calls: unknown[][] }; mockRestore(): void };
  let uninstall: (() => void) | undefined;

  beforeEach(() => {
    clearLogs();
    errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    uninstall?.();
    uninstall = undefined;
    vi.restoreAllMocks();
    clearLogs();
  });

  it('mirrors console.error and console.warn and still calls the originals', () => {
    uninstall = installLogCapture();
    console.error('bad', 7);
    console.warn('careful');

    expect(getRecentLogs().map((entry) => [entry.level, entry.message])).toEqual([
      ['error', 'bad 7'],
      ['warn', 'careful'],
    ]);
    expect(errorSpy.mock.calls).toEqual([['bad', 7]]);
  });

  it('restores the original console methods on uninstall', () => {
    const originalError = console.error;
    uninstall = installLogCapture();
    expect(console.error).not.toBe(originalError);

    uninstall();
    uninstall = undefined;
    expect(console.error).toBe(originalError);
    console.error('after');
    expect(getRecentLogs()).toHaveLength(0);
  });

  it('is idempotent: a second install returns the same uninstall and records once', () => {
    const first = installLogCapture();
    const second = installLogCapture();
    expect(second).toBe(first);
    uninstall = first;

    console.error('once');
    expect(getRecentLogs()).toHaveLength(1);
  });

  it('records unhandled promise rejections', () => {
    uninstall = installLogCapture();
    const event = new Event('unhandledrejection');
    Object.assign(event, { reason: 'lost promise' });
    window.dispatchEvent(event);

    const [entry] = getRecentLogs();
    expect(entry.level).toBe('rejection');
    expect(entry.message).toBe('lost promise');
  });
});
