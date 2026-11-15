import { beforeEach, describe, expect, it, vi } from 'vitest';
import React from 'react';
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { commands } from '../../api/commands';
import { SNAPSHOT_QUERY_KEY } from '../../api/useSnapshot';
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryView,
  type ViewWriteView,
} from '../../api/types';
import { I18nProvider } from '../../i18n';
import {
  closeSaveView,
  openRenameView,
  openSaveView,
  resetUiStore,
  uiStore,
} from '../../store/uiStore';
import { SaveViewModal } from './SaveViewModal';

function makeLibrary(overrides: Partial<LibraryView> = {}): LibraryView {
  const filters = {
    ...createDefaultFilters(),
    availability: { installed: true, owned: false, shared: false },
    stores: ['steam'],
    playtime: 'atLeast10h' as const,
    played: 'played' as const,
    lastPlayed: 'last30Days' as const,
    release: { decade: 2020 },
    added: 'last30Days' as const,
    favoritesOnly: true,
  };
  return {
    ...createDefaultLibraryView(),
    generation: 2,
    access: 'verified',
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 5,
        installedCount: 5,
        ownedCount: 5,
        sharedCount: 0,
        sharedLabel: null,
      },
    ],
    savedViews: [
      { id: 5, name: 'Steam 10 h+', count: 5, active: false },
      { id: 6, name: 'Backlog', count: 2, active: false },
    ],
    collections: [{ id: 'backlog-curto', name: 'Backlog curto', count: 4 }],
    canSaveView: true,
    layout: {
      ...createDefaultLibraryLayout(),
      scope: 'all',
      filters,
      sort: { key: 'lastPlayed', direction: 'descending' },
      group: 'store',
      view: 'list',
    },
    ...overrides,
  };
}

function viewWrite(overrides: Partial<ViewWriteView> = {}): ViewWriteView {
  return {
    run: 1,
    operation: 'save',
    viewId: 0,
    name: 'My Steam view',
    state: 'pending',
    error: null,
    savedViewId: null,
    ...overrides,
  };
}

function wrap(
  ui: React.ReactElement,
  locale: 'en' | 'pt-BR' = 'en',
  queryClient = createAppQueryClient(),
) {
  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>
  );
}

async function submitSave(user: ReturnType<typeof userEvent.setup>, name = 'My Steam view') {
  const input = screen.getByTestId('save-view-name');
  await user.clear(input);
  await user.type(input, name);
  await user.click(screen.getByRole('button', { name: 'Save view' }));
}

function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('SaveViewModal', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('shows the includes summary for scope, filters, sort, group and view', () => {
    act(() => openSaveView());
    render(wrap(<SaveViewModal library={makeLibrary()} />));

    expect(screen.getByTestId('save-view-modal')).toBeInTheDocument();
    const summary = screen.getByTestId('save-view-summary');
    expect(within(summary).getByText('All games')).toBeInTheDocument();
    expect(within(summary).getByText('Availability: Installed')).toBeInTheDocument();
    expect(within(summary).getByText('Store: Steam')).toBeInTheDocument();
    expect(within(summary).getByText('Playtime: 10 h+')).toBeInTheDocument();
    expect(within(summary).getByText('History: Played')).toBeInTheDocument();
    expect(within(summary).getByText('Last session: 30 days')).toBeInTheDocument();
    expect(within(summary).getByText('Release: 2020s')).toBeInTheDocument();
    expect(within(summary).getByText('Added: This month')).toBeInTheDocument();
    expect(within(summary).getByText('Favorites only')).toBeInTheDocument();
    expect(within(summary).getByText('Sort: Recently played')).toBeInTheDocument();
    expect(within(summary).getByText('Group: Store')).toBeInTheDocument();
    expect(within(summary).getByText('List')).toBeInTheDocument();
    expect(
      screen.getByText('Appears under “Collections and views” in the sidebar'),
    ).toBeInTheDocument();
  });

  it('keeps the raw 340 px dialog width instead of Mantine rem scaling', () => {
    act(() => openSaveView());
    render(wrap(<SaveViewModal library={makeLibrary()} />));

    const modal = screen.getByTestId('save-view-modal');
    expect(modal).toHaveClass('w-[340px]', 'max-w-full', 'p-[16px]');
    expect(modal.querySelector('form')?.className).toContain('gap-[12px]');
    const sized = modal.closest('[style*="--modal-size"]');
    expect(sized?.getAttribute('style')).toContain('--modal-size: calc(340px)');
  });

  it('saves a trimmed name and closes only after viewWrite confirms the persisted id', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await user.type(screen.getByTestId('save-view-name'), '  My Steam view  ');
    await user.click(screen.getByRole('button', { name: 'Save view' }));

    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith({ saveView: { name: 'My Steam view' } });
    expect(screen.getByTestId('save-view-modal')).toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);

    // `lastSavedView` drives the notification only; it is not a write acknowledgement.
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            lastSavedView: { id: 9, name: 'My Steam view', run: 1 },
          }}
        />,
      ),
    );
    await act(async () => {});
    expect(uiStore.state.saveViewOpen).toBe(true);

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 1,
              state: 'succeeded',
              savedViewId: 9,
            }),
            lastSavedView: { id: 9, name: 'My Steam view', run: 1 },
          }}
        />,
      ),
    );

    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
    expect(screen.queryByTestId('save-view-modal')).not.toBeInTheDocument();
  });

  it('renames an existing view from the default layout even when canSaveView is false', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary({
      canSaveView: false,
      layout: {
        ...createDefaultLibraryLayout(),
        filters: createDefaultFilters(),
      },
    });
    act(() => openRenameView(5));
    const view = render(wrap(<SaveViewModal library={library} />));

    const input = screen.getByTestId('save-view-name');
    expect(input).toHaveValue('Steam 10 h+');
    expect(screen.getByRole('button', { name: 'Rename view' })).toBeDisabled();

    await user.clear(input);
    await user.type(input, '  My Steam view  ');
    const button = screen.getByRole('button', { name: 'Rename view' });
    expect(button).toBeEnabled();
    await user.click(button);

    expect(spy).toHaveBeenCalledWith({
      renameView: { id: 5, name: 'My Steam view' },
    });

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            savedViews: library.savedViews.map((candidate) =>
              candidate.id === 5 ? { ...candidate, name: 'My Steam view' } : candidate,
            ),
            viewWrite: viewWrite({
              run: 4,
              operation: 'rename',
              viewId: 5,
              state: 'succeeded',
              savedViewId: 5,
            }),
          }}
        />,
      ),
    );

    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('does not create a new view from the default layout', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary({
      canSaveView: false,
      layout: {
        ...createDefaultLibraryLayout(),
        filters: createDefaultFilters(),
      },
    });
    act(() => openSaveView());
    render(wrap(<SaveViewModal library={library} />));

    await user.type(screen.getByTestId('save-view-name'), 'My Steam view');
    const button = screen.getByRole('button', { name: 'Save view' });
    expect(button).toBeDisabled();
    await user.click(button);
    expect(spy).not.toHaveBeenCalled();
  });

  it('stops the spinner on a failed save and retries without closing', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    const pending = viewWrite({ run: 1, state: 'pending' });
    view.rerender(wrap(<SaveViewModal library={{ ...library, viewWrite: pending }} />));

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 1, state: 'failed', error: 'failed' }),
          }}
        />,
      ),
    );

    expect(await screen.findByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    const button = screen.getByRole('button', { name: 'Save view' });
    expect(button).toBeEnabled();
    expect(button).not.toHaveAttribute('aria-busy');
    expect(uiStore.state.saveViewOpen).toBe(true);

    await user.click(button);
    expect(spy).toHaveBeenCalledTimes(2);

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 2, state: 'succeeded', savedViewId: 9 }),
          }}
        />,
      ),
    );
    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('stops the spinner on a failed rename and retries', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openRenameView(5));
    const view = render(wrap(<SaveViewModal library={library} />));

    await user.clear(screen.getByTestId('save-view-name'));
    await user.type(screen.getByTestId('save-view-name'), 'My Steam view');
    await user.click(screen.getByRole('button', { name: 'Rename view' }));
    expect(spy).toHaveBeenCalledTimes(1);

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 1,
              operation: 'rename',
              viewId: 5,
              state: 'failed',
              error: 'failed',
            }),
          }}
        />,
      ),
    );

    expect(await screen.findByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Rename view' })).toBeEnabled();
    expect(uiStore.state.saveViewOpen).toBe(true);

    await user.click(screen.getByRole('button', { name: 'Rename view' }));
    expect(spy).toHaveBeenCalledTimes(2);

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 2,
              operation: 'rename',
              viewId: 5,
              state: 'succeeded',
              savedViewId: 5,
            }),
          }}
        />,
      ),
    );
    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('stops the spinner when the IPC rejects and succeeds on a retry', async () => {
    const user = userEvent.setup();
    const spy = vi
      .spyOn(commands, 'libraryInput')
      .mockRejectedValueOnce(new Error('ipc down'))
      .mockResolvedValue(undefined);
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);

    expect(await screen.findByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    const button = screen.getByRole('button', { name: 'Save view' });
    expect(button).toBeEnabled();
    expect(uiStore.state.saveViewOpen).toBe(true);

    await user.click(button);
    expect(spy).toHaveBeenCalledTimes(2);

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 1, state: 'succeeded', savedViewId: 9 }),
          }}
        />,
      ),
    );
    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('reacts to a repeated failure on a new run', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 1, state: 'failed', error: 'failed' }),
          }}
        />,
      ),
    );
    expect(await screen.findByText("Couldn't save the view. Try again.")).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Save view' }));
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 2, state: 'pending' }),
          }}
        />,
      ),
    );
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 2, state: 'failed', error: 'failed' }),
          }}
        />,
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save view' })).toBeEnabled(),
    );
    expect(screen.getByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('shows an engine duplicate refusal and retries on a new run', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user, 'Backlog novo');
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 1,
              name: 'Backlog novo',
              state: 'failed',
              error: 'duplicate',
            }),
          }}
        />,
      ),
    );
    expect(
      await screen.findByText('A view with this name already exists'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save view' })).toBeEnabled();

    await user.click(screen.getByRole('button', { name: 'Save view' }));
    expect(spy).toHaveBeenCalledTimes(2);
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 2,
              name: 'Backlog novo',
              state: 'failed',
              error: 'duplicate',
            }),
          }}
        />,
      ),
    );
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save view' })).toBeEnabled(),
    );
    expect(screen.getByText('A view with this name already exists')).toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('ignores stale, foreign or unconfirmed results while the attempt is current', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);

    // A result at the baseline belongs to an earlier attempt.
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 0, state: 'succeeded', savedViewId: 8 }),
          }}
        />,
      ),
    );
    // A result for another operation and name is not this attempt's answer.
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 4,
              operation: 'rename',
              viewId: 5,
              name: 'Other view',
              state: 'succeeded',
              savedViewId: 5,
            }),
          }}
        />,
      ),
    );
    // A success without the persisted id is never treated as a confirmation.
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 4, state: 'succeeded', savedViewId: null }),
          }}
        />,
      ),
    );
    await act(async () => {});
    expect(uiStore.state.saveViewOpen).toBe(true);
    // A success without the persisted id is reported as a retryable failure, never a close.
    expect(screen.getByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save view' })).toBeEnabled();
  });

  it('ignores a failed result from before this opening', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary({
      viewWrite: viewWrite({ run: 7, state: 'failed', error: 'failed' }),
    });
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    expect(
      screen.queryByText("Couldn't save the view. Try again."),
    ).not.toBeInTheDocument();

    await submitSave(user);
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 8, state: 'succeeded', savedViewId: 9 }),
          }}
        />,
      ),
    );
    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('stops saving when the pending attempt is cleared by an account change', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 1, state: 'pending' }),
          }}
        />,
      ),
    );
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    view.rerender(
      wrap(<SaveViewModal library={{ ...library, viewWrite: null }} />),
    );

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save view' })).toBeEnabled(),
    );
    expect(screen.getByText("Couldn't save the view. Try again.")).toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('ends an attempt on an account switch before the first pending and ignores the old result', async () => {
    const user = userEvent.setup();
    const gate = deferred();
    const spy = vi
      .spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined)
      .mockImplementationOnce(() => gate.promise);
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} accountId="ana" />));

    await submitSave(user);
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    // The account switches before the engine ever publishes `pending`, and the new account's
    // same-operation, same-name success arrives in the same snapshot. It belongs to another
    // account's write and must neither close the dialog nor look like this attempt's answer.
    view.rerender(
      wrap(
        <SaveViewModal
          accountId="bo"
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 1,
              name: 'My Steam view',
              state: 'succeeded',
              savedViewId: 9,
            }),
          }}
        />,
      ),
    );
    await act(async () => {});

    expect(screen.getByTestId('save-view-modal')).toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);
    expect(screen.getByRole('button', { name: 'Save view' })).not.toHaveAttribute(
      'aria-busy',
    );
    expect(screen.queryByText("Couldn't save the view. Try again.")).not.toBeInTheDocument();
    // The form belongs to the new account, so the old account's typed name is gone; the empty
    // name (not the old spinner) is why Save stays disabled until a name is typed again.
    expect(screen.getByTestId('save-view-name')).toHaveValue('');

    // Rejecting the old account's IPC call later must not paint an error either.
    await act(async () => {
      gate.reject(new Error('old account ipc'));
      await gate.promise.catch(() => undefined);
    });
    await act(async () => {});
    expect(screen.queryByText("Couldn't save the view. Try again.")).not.toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);

    // The new account can still save the same name and closes on its own confirmation.
    await user.type(screen.getByTestId('save-view-name'), 'My Steam view');
    const retry = screen.getByRole('button', { name: 'Save view' });
    expect(retry).toBeEnabled();
    await user.click(retry);
    expect(spy).toHaveBeenCalledTimes(2);

    view.rerender(
      wrap(
        <SaveViewModal
          accountId="bo"
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 2,
              name: 'My Steam view',
              state: 'succeeded',
              savedViewId: 10,
            }),
          }}
        />,
      ),
    );
    await waitFor(() => expect(uiStore.state.saveViewOpen).toBe(false));
  });

  it('ends an attempt on sign-out before the first pending and ignores the late rejection', async () => {
    const user = userEvent.setup();
    const gate = deferred();
    vi.spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined)
      .mockImplementationOnce(() => gate.promise);
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} accountId="ana" />));

    await submitSave(user);
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    view.rerender(
      wrap(<SaveViewModal accountId={null} library={{ ...library, viewWrite: null }} />),
    );
    await act(async () => {});

    expect(screen.getByRole('button', { name: 'Save view' })).not.toHaveAttribute(
      'aria-busy',
    );
    expect(screen.getByTestId('save-view-modal')).toBeInTheDocument();
    expect(screen.getByTestId('save-view-name')).toHaveValue('');
    expect(screen.queryByText("Couldn't save the view. Try again.")).not.toBeInTheDocument();

    await act(async () => {
      gate.reject(new Error('signed out ipc'));
      await gate.promise.catch(() => undefined);
    });
    await act(async () => {});

    expect(screen.queryByText("Couldn't save the view. Try again.")).not.toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('binds the attempt to the account in the snapshot auth', async () => {
    const user = userEvent.setup();
    const gate = deferred();
    vi.spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined)
      .mockImplementationOnce(() => gate.promise);
    const library = makeLibrary();
    const queryClient = createAppQueryClient();
    const snapshot = createDefaultSnapshot();
    snapshot.auth = { signedIn: { user: { id: 'ana' } } };
    queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

    act(() => openSaveView());
    render(wrap(<SaveViewModal library={library} />, 'en', queryClient));

    await submitSave(user);
    expect(screen.getByRole('button', { name: 'Save view' })).toHaveAttribute(
      'aria-busy',
      'true',
    );

    // The real auth selection, not a test-only prop, identifies the account change.
    const next = createDefaultSnapshot();
    next.auth = { signedIn: { user: { id: 'bo' } } };
    act(() => queryClient.setQueryData(SNAPSHOT_QUERY_KEY, next));

    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Save view' })).not.toHaveAttribute(
        'aria-busy',
      ),
    );
    expect(screen.getByTestId('save-view-name')).toHaveValue('');
    expect(screen.queryByText("Couldn't save the view. Try again.")).not.toBeInTheDocument();

    await act(async () => {
      gate.resolve();
      await gate.promise;
    });
    await act(async () => {});
    expect(uiStore.state.saveViewOpen).toBe(true);
  });

  it('never reopens or closes from a result after the dialog was cancelled', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />));

    await submitSave(user);
    act(() => closeSaveView());
    expect(screen.queryByTestId('save-view-modal')).not.toBeInTheDocument();

    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({ run: 1, state: 'succeeded', savedViewId: 9 }),
          }}
        />,
      ),
    );
    await act(async () => {});
    expect(screen.queryByTestId('save-view-modal')).not.toBeInTheDocument();
    expect(uiStore.state.saveViewOpen).toBe(false);
  });

  it('blocks empty and case-insensitive duplicate names live', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    act(() => openSaveView());
    render(wrap(<SaveViewModal library={makeLibrary()} />));

    const input = screen.getByTestId('save-view-name');
    await user.type(input, '   ');
    expect(input).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByRole('button', { name: 'Save view' })).toBeDisabled();

    await user.clear(input);
    await user.type(input, 'steam 10 h+');
    expect(
      screen.getByText('A view with this name already exists'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Save view' })).toBeDisabled();

    await user.click(screen.getByRole('button', { name: 'Save view' }));
    expect(spy).not.toHaveBeenCalled();
  });

  it('clears the local validation state when the dialog is reopened', async () => {
    const user = userEvent.setup();
    act(() => openSaveView());
    render(wrap(<SaveViewModal library={makeLibrary()} />));

    await user.type(screen.getByTestId('save-view-name'), 'steam 10 h+');
    expect(screen.getByTestId('save-view-validation')).toBeInTheDocument();

    act(() => closeSaveView());
    expect(screen.queryByTestId('save-view-modal')).not.toBeInTheDocument();

    act(() => openSaveView());
    expect(screen.getByTestId('save-view-name')).toHaveValue('');
    expect(screen.queryByTestId('save-view-validation')).not.toBeInTheDocument();
  });

  it('renders the save dialog in pt-BR, including the write failure', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary();
    act(() => openSaveView());
    const view = render(wrap(<SaveViewModal library={library} />, 'pt-BR'));

    expect(
      screen.getByRole('heading', { name: 'Salvar como visão' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Inclui')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Salvar visão' })).toBeInTheDocument();
    expect(
      screen.getByText('Aparece em “Coleções e visões” na barra lateral'),
    ).toBeInTheDocument();

    await user.type(screen.getByTestId('save-view-name'), 'Minha visão');
    await user.click(screen.getByRole('button', { name: 'Salvar visão' }));
    view.rerender(
      wrap(
        <SaveViewModal
          library={{
            ...library,
            viewWrite: viewWrite({
              run: 1,
              name: 'Minha visão',
              state: 'failed',
              error: 'failed',
            }),
          }}
        />,
        'pt-BR',
      ),
    );
    expect(
      await screen.findByText('Não foi possível salvar a visão. Tente novamente.'),
    ).toBeInTheDocument();
  });

  it('renders the rename title and prefilled name in rename mode', () => {
    act(() => openRenameView(6));
    render(wrap(<SaveViewModal library={makeLibrary()} />));

    expect(screen.getByRole('heading', { name: 'Rename view' })).toBeInTheDocument();
    expect(screen.getByTestId('save-view-name')).toHaveValue('Backlog');
  });
});
