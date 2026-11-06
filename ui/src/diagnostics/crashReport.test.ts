import { describe, expect, it } from 'vitest';
import {
  buildCrashReport,
  describeError,
  failingComponent,
  type CrashDetails,
} from './crashReport';
import type { LogEntry } from './logBuffer';

const env = { userAgent: 'TestAgent/1.0', language: 'en-US' };

const logs: LogEntry[] = [
  { time: '2026-11-06T10:00:00.000Z', level: 'warn', message: 'loaded 3 games' },
  { time: '2026-11-06T10:00:01.000Z', level: 'error', message: 'cover missing' },
];

function details(overrides: Partial<CrashDetails> = {}): CrashDetails {
  return {
    scope: 'game-tools',
    error: new TypeError('Cannot read properties of null'),
    componentStack: '\n    at GameToolsModal (http://app/assets/index.js:1:2)\n    at App (http://app/assets/index.js:3:4)',
    capturedAt: '2026-11-06T10:00:02.000Z',
    logs,
    context: { 'Game entry': 'steam:1' },
    ...overrides,
  };
}

describe('describeError', () => {
  it('uses the name and message of an Error, and keeps its stack', () => {
    const summary = describeError(new TypeError('nope'));
    expect(summary.headline).toBe('TypeError: nope');
    expect(summary.stack).toContain('TypeError: nope');
  });

  it('describes a thrown non-Error value by its text', () => {
    expect(describeError('plain failure')).toEqual({ headline: 'plain failure', stack: null });
  });
});

describe('failingComponent', () => {
  it('returns the innermost component named in the component stack', () => {
    expect(failingComponent(details().componentStack)).toBe('GameToolsModal');
  });

  it('returns null when React gave no stack', () => {
    expect(failingComponent(null)).toBeNull();
    expect(failingComponent('no frames here')).toBeNull();
  });
});

describe('buildCrashReport', () => {
  it('lists what happened, where, the context and the recent log, oldest first', () => {
    const report = buildCrashReport(details(), env);

    expect(report).toContain('GameNode Desktop crash report');
    expect(report).toContain('Captured:   2026-11-06T10:00:02.000Z');
    expect(report).toContain('Boundary:   game-tools');
    expect(report).toContain('Game entry: steam:1');
    expect(report).toContain('Language:   en-US');
    expect(report).toContain('User agent: TestAgent/1.0');
    expect(report).toContain('What happened: TypeError: Cannot read properties of null');
    expect(report).toMatch(/Where:\s+GameToolsModal/);
    expect(report).toContain('== Recent log (2 entries, oldest first) ==');

    const warnIndex = report.indexOf('[warn] loaded 3 games');
    const errorIndex = report.indexOf('[error] cover missing');
    expect(warnIndex).toBeGreaterThan(-1);
    expect(errorIndex).toBeGreaterThan(warnIndex);
  });

  it('says so when there are no log entries or no component stack', () => {
    const report = buildCrashReport(details({ logs: [], componentStack: null }), env);
    expect(report).toContain('== Recent log (0 entries, oldest first) ==');
    expect(report).toContain('(no entries)');
    expect(report).toContain('Not reported by React.');
    expect(report).not.toContain('Where:');
  });
});
