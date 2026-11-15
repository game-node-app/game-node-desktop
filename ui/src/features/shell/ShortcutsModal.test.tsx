import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import { openShortcuts, resetUiStore, uiStore } from '../../store/uiStore';
import { ShortcutsModal } from './ShortcutsModal';

function wrap(ui: React.ReactElement, locale: 'en' | 'pt-BR' = 'en') {
  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={createAppQueryClient()}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>
  );
}

describe('ShortcutsModal', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('lists the General, Library and Selected game shortcuts', () => {
    render(wrap(<ShortcutsModal open onClose={() => undefined} />));

    expect(screen.getByTestId('shortcuts-modal')).toBeInTheDocument();
    expect(screen.getByText('Keyboard shortcuts')).toBeInTheDocument();
    expect(screen.getByText('General')).toBeInTheDocument();
    expect(screen.getByText('Command palette')).toBeInTheDocument();
    expect(screen.getByText('Library')).toBeInTheDocument();
    expect(screen.getByText('Filters')).toBeInTheDocument();
    expect(screen.getByText('Selected game')).toBeInTheDocument();
    expect(screen.getByText('Context menu')).toBeInTheDocument();
    expect(screen.getByText('Select several')).toBeInTheDocument();
    expect(screen.getByText('Select all')).toBeInTheDocument();

    const general = screen.getByTestId('shortcuts-section-general');
    expect(general).toHaveTextContent('Ctrl');
    expect(general).toHaveTextContent('K');
    const selected = screen.getByTestId('shortcuts-section-selected');
    expect(selected).toHaveTextContent('Shift');
    expect(selected).toHaveTextContent('F10');
    expect(selected).toHaveTextContent('Click');
  });

  it('opens through the UI store when mounted without props', () => {
    render(wrap(<ShortcutsModal />));
    expect(screen.queryByTestId('shortcuts-modal')).not.toBeInTheDocument();

    act(() => openShortcuts());
    expect(screen.getByTestId('shortcuts-modal')).toBeInTheDocument();
    expect(uiStore.state.shortcutsOpen).toBe(true);
  });

  it('closes with Escape and restores focus to the trigger', async () => {
    const user = userEvent.setup();

    function Harness() {
      return <ShortcutsModal />;
    }

    render(
      wrap(
        <>
          <button type="button" onClick={openShortcuts}>
            Open shortcuts
          </button>
          <Harness />
        </>,
      ),
    );
    const trigger = screen.getByRole('button', { name: 'Open shortcuts' });
    await user.click(trigger);
    expect(screen.getByTestId('shortcuts-modal')).toBeInTheDocument();

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(screen.queryByTestId('shortcuts-modal')).not.toBeInTheDocument(),
    );
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('renders the pt-BR section copy', () => {
    render(wrap(<ShortcutsModal open onClose={() => undefined} />, 'pt-BR'));
    expect(screen.getByText('Atalhos de teclado')).toBeInTheDocument();
    expect(screen.getByText('Geral')).toBeInTheDocument();
    expect(screen.getByText('Biblioteca')).toBeInTheDocument();
    expect(screen.getByText('Jogo selecionado')).toBeInTheDocument();
    expect(screen.getByTestId('shortcuts-section-selected')).toHaveTextContent('Clique');
  });
});
