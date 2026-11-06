import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ToastContainer } from './ToastContainer';
import { pushToast, resetUiStore, uiStore } from '../../store/uiStore';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';

function renderToasts() {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale="en">
        <ToastContainer />
      </I18nProvider>
    </MantineProvider>,
  );
}

describe('ToastContainer', () => {
  beforeEach(() => {
    resetUiStore();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('dismisses a toast through its close button', () => {
    pushToast('success', 'Saved');
    renderToasts();

    expect(screen.getByText('Saved')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    expect(screen.queryByText('Saved')).toBeNull();
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('auto-closes the oldest toast after four seconds', () => {
    pushToast('success', 'First');
    pushToast('danger', 'Second');
    renderToasts();

    expect(screen.getAllByRole('status')).toHaveLength(2);

    act(() => {
      vi.advanceTimersByTime(4000);
    });

    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0].message).toBe('Second');
    expect(screen.queryByText('First')).toBeNull();
  });
});
