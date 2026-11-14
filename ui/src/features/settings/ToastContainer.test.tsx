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

  it('renders a plugin toast as attributed plain text, never as markup or a link', () => {
    const hostile = '<script>alert(1)</script><img src=x onerror=alert(2)>';
    pushToast('warning', hostile, {
      body: '[click](javascript:alert(3))\n<a href="https://evil.test">pay</a> https://evil.test/x',
      source: '<b>org.example.plugin</b>',
    });
    const { container } = renderToasts();

    // The strings are on screen as text.
    expect(screen.getByText(hostile)).toBeInTheDocument();
    expect(screen.getByText('From plugin <b>org.example.plugin</b>')).toBeInTheDocument();
    expect(container.textContent).toContain('<a href="https://evil.test">pay</a>');
    // Nothing of it became an element: no script, image, link or bold text anywhere.
    for (const tag of ['script', 'img', 'a', 'b', 'iframe', 'svg[onload]']) {
      expect(document.querySelector(tag), tag).toBeNull();
    }
    expect(document.querySelectorAll('[href], [src], [onerror], [onclick]')).toHaveLength(0);
    // The line break in the body is kept.
    expect(screen.getByText(/\[click\]\(javascript:alert\(3\)\)/).className).toContain(
      'whitespace-pre-line',
    );
  });

  it('keeps a plugin toast for its own duration', () => {
    pushToast('info', 'Slow', { durationMs: 10000 });
    pushToast('info', 'Next');
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(9999);
    });
    expect(screen.getByText('Slow')).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByText('Slow')).toBeNull();
  });

  it('has a look for every kind', () => {
    for (const kind of ['success', 'danger', 'info', 'warning'] as const) {
      pushToast(kind, `message ${kind}`);
    }
    renderToasts();
    expect(screen.getAllByRole('status')).toHaveLength(4);
  });
});
