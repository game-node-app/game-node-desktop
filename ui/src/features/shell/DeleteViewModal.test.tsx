import { beforeEach, describe, expect, it, vi } from 'vitest';
import React, { useState } from 'react';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { commands } from '../../api/commands';
import {
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  type LibraryView,
} from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { DeleteViewModal } from './DeleteViewModal';

function makeLibrary(): LibraryView {
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    savedViews: [{ id: 5, name: 'Steam 10 h+', count: 5, active: false }],
    layout: createDefaultLibraryLayout(),
  };
}

function wrap(ui: React.ReactElement) {
  const queryClient = createAppQueryClient();
  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale="en">{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>
  );
}

describe('DeleteViewModal', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('names the view and cancels without deleting', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const onClose = vi.fn();
    render(
      wrap(
        <DeleteViewModal
          open
          viewId={5}
          library={makeLibrary()}
          onClose={onClose}
        />,
      ),
    );

    expect(screen.getByTestId('delete-view-modal')).toHaveTextContent(
      'Delete view “Steam 10 h+”',
    );
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(spy).not.toHaveBeenCalled();
  });

  it('confirms delete with the real input and closes', async () => {
    const user = userEvent.setup();
    const spy = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const onClose = vi.fn();
    render(
      wrap(
        <DeleteViewModal
          open
          viewId={5}
          library={makeLibrary()}
          onClose={onClose}
        />,
      ),
    );

    await user.click(screen.getByTestId('delete-view-confirm'));
    expect(spy).toHaveBeenCalledWith({ deleteView: { id: 5 } });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('uses the AppModal layer for Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();
    const library = makeLibrary();

    function Harness() {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setOpen(true)}>
            Open delete
          </button>
          <DeleteViewModal
            open={open}
            viewId={5}
            library={library}
            onClose={() => setOpen(false)}
          />
        </>
      );
    }

    render(wrap(<Harness />));
    const trigger = screen.getByRole('button', { name: 'Open delete' });
    await user.click(trigger);
    expect(screen.getByTestId('delete-view-modal')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(screen.queryByTestId('delete-view-modal')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
