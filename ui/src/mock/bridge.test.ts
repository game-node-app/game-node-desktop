import { afterEach, expect, it, vi } from 'vitest';
import type { Snapshot } from '../api/types';
import { createAppQueryClient } from '../AppProviders';

afterEach(() => { vi.unstubAllEnvs(); vi.resetModules(); });

it('routes commands and snapshot events through the mock even when Tauri is present', async () => {
  // `AppProviders` is imported statically and evaluates the mock switch eagerly,
  // before the env stub below, so drop the cached modules once inside the test.
  vi.resetModules();
  vi.stubEnv('VITE_UI_MOCK', '1');
  const internals = (window as unknown as Record<string, unknown>);
  internals.__TAURI_INTERNALS__ = {};
  try {
    const { commands } = await import('../api/commands');
    const { SNAPSHOT_QUERY_KEY, subscribeToEngineSnapshots } = await import('../api/useSnapshot');
    const client = createAppQueryClient();
    const unlisten = await subscribeToEngineSnapshots(client);
    const initial = await commands.getSnapshot();
    expect(initial.library?.rows).toHaveLength(12);
    await commands.libraryInput({ setSearch: 'Aster' });
    expect(client.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY)?.library?.rows.map(row => row.title)).toEqual(['Aster Vale']);
    await commands.dispatchAuth('signOut');
    expect(client.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY)?.auth).toBe('signedOut');
    unlisten();
  } finally {
    delete internals.__TAURI_INTERNALS__;
  }
});

it('routes PC ports commands and progress events through the mock', async () => {
  vi.resetModules();
  vi.stubEnv('VITE_UI_MOCK', '1');
  const internals = (window as unknown as Record<string, unknown>);
  internals.__TAURI_INTERNALS__ = {};
  vi.useFakeTimers();
  try {
    const { commands } = await import('../api/commands');
    const { SNAPSHOT_QUERY_KEY, subscribeToEngineSnapshots } = await import('../api/useSnapshot');
    const { subscribeToPortProgress } = await import('../api/ports');

    const client = createAppQueryClient();
    const unlistenSnapshot = await subscribeToEngineSnapshots(client);
    const phases: string[] = [];
    const unlistenProgress = await subscribeToPortProgress((event) => {
      phases.push(event.phase);
    });

    await commands.portsSetPreferences({ view: 'list' });
    expect(
      client.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY)?.ports.preferences.view,
    ).toBe('list');

    const jobId = await commands.portsInstall({
      catalogId: 'qcat_mock_beyond',
      parentFolder: 'D:\\Ports',
      supplied: [],
    });
    expect(jobId).toMatch(/^job-mock-/);
    // The mock emits the first downloading tick synchronously.
    expect(phases).toContain('downloading');

    await commands.portsCancel(jobId as string);
    vi.advanceTimersByTime(300);
    expect(
      client
        .getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY)
        ?.ports.jobs.find((job) => job.jobId === jobId)?.phase,
    ).toBe('cancelled');

    unlistenProgress();
    unlistenSnapshot();
  } finally {
    vi.useRealTimers();
    delete internals.__TAURI_INTERNALS__;
  }
});
