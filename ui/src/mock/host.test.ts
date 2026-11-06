import { describe, expect, it, vi } from 'vitest';
import { createMockHost } from './host';
import type { BootstrapState, Snapshot } from '../api/types';

describe('UI mock host', () => {
  it('starts with a verified mock session and deterministic games', async () => {
    const host = createMockHost();
    const state = await host.invoke('get_bootstrap_state') as BootstrapState;
    expect(state.snapshot.auth).toEqual({ signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } });
    expect(state.snapshot.library?.access).toBe('verified');
    expect(state.snapshot.library?.rows).toHaveLength(12);
    state.snapshot.library!.rows.length = 0;
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
  });

  it('publishes interactions and resets on sign out and sign in', async () => {
    const host = createMockHost({ signedOut: true, locale: 'pt-BR' });
    expect((await host.invoke('get_bootstrap_state') as BootstrapState).locale).toBe('pt-BR');
    expect((await host.invoke('get_snapshot') as Snapshot).auth).toBe('signedOut');
    const listener = vi.fn();
    const unsubscribe = host.subscribe(listener);
    await host.invoke('dispatch_auth', { input: { startSignIn: 'google' } });
    await host.invoke('library_input', { input: { setSearch: 'aster' } });
    let snapshot = await host.invoke('get_snapshot') as Snapshot;
    expect(snapshot.library?.rows.map(row => row.title)).toEqual(['Aster Vale']);
    await host.invoke('library_input', { input: { select: { key: 'steam:1', mode: 'replace' } } });
    await host.invoke('library_input', { input: { setDetailOpen: true } });
    snapshot = listener.mock.lastCall![0] as Snapshot;
    expect(snapshot.library?.detail?.row.title).toBe('Aster Vale');
    expect(snapshot.library?.layout.detailOpen).toBe(true);
    await host.invoke('dispatch_auth', { input: 'signOut' });
    await host.invoke('dispatch_auth', { input: { startSignIn: 'discord' } });
    expect((await host.invoke('get_snapshot') as Snapshot).library?.rows).toHaveLength(12);
    unsubscribe();
    listener.mockClear();
    await host.invoke('set_sync_enabled', { enabled: false });
    expect(listener).not.toHaveBeenCalled();
  });

  it('does not route unsupported commands to any real service', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const host = createMockHost();
    await host.invoke('open_entry', { entryKey: 'steam:1', kind: 'play' });
    expect(warn).toHaveBeenCalledWith('[UI mock] Unsupported command: open_entry');
    warn.mockRestore();
  });
});
