import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ToastContainer } from './ToastContainer';
import {
  pushNotification,
  pushToast,
  resetUiStore,
  uiStore,
  updateNotification,
} from '../../store/uiStore';
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
    vi.restoreAllMocks();
  });

  it('announces notifications in the bottom-right region with title, body and actions', () => {
    pushNotification({
      kind: 'success',
      title: '3 games added to GameNode',
      body: 'Status: Playing',
      persistent: true,
      actions: [
        { label: 'Undo', onClick: () => undefined },
        { label: 'View on GameNode', onClick: () => undefined },
      ],
    });
    renderToasts();

    const region = screen.getByRole('region', { name: 'Notifications' });
    expect(region).toHaveAttribute('aria-live', 'polite');
    expect(screen.getByRole('status')).toHaveAttribute('aria-live', 'polite');
    expect(screen.getByText('3 games added to GameNode')).toBeInTheDocument();
    expect(screen.getByText('Status: Playing')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Undo' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'View on GameNode' })).toBeInTheDocument();
  });

  it('dismisses through the accessible close button', () => {
    pushNotification({ kind: 'danger', title: 'Could not start the game' });
    renderToasts();

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    expect(uiStore.state.toasts).toHaveLength(0);
    expect(screen.queryByText('Could not start the game')).toBeNull();
  });

  it('caps the visible actions at two', () => {
    pushNotification({
      kind: 'danger',
      title: 'Notice',
      actions: [
        { label: 'First', onClick: () => undefined },
        { label: 'Second', onClick: () => undefined },
        { label: 'Third', onClick: () => undefined },
      ],
    });
    renderToasts();

    expect(screen.getByRole('button', { name: 'First' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Second' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Third' })).toBeNull();
  });

  it('auto-closes success and info after six seconds', () => {
    pushNotification({ kind: 'success', title: 'Saved' });
    pushNotification({ kind: 'info', title: 'Heads up' });
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(5999);
    });
    expect(uiStore.state.toasts).toHaveLength(2);

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('keeps danger and progress until they are dismissed or finished', () => {
    pushNotification({ kind: 'danger', title: 'Could not update GameNode' });
    pushNotification({ kind: 'progress', title: 'Downloading OptiScaler', progress: 64 });
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(60000);
    });

    expect(uiStore.state.toasts).toHaveLength(2);
    expect(screen.getByText('Could not update GameNode')).toBeInTheDocument();
    expect(screen.getByText('Downloading OptiScaler')).toBeInTheDocument();
  });

  it('honors the persistent flag on success and info', () => {
    pushNotification({ kind: 'success', title: 'Read this later', persistent: true });
    pushNotification({ kind: 'info', title: 'Also read this', persistent: true });
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(60000);
    });

    expect(uiStore.state.toasts).toHaveLength(2);
  });

  it('pauses on hover and resumes with the remaining time', () => {
    pushNotification({ kind: 'success', title: 'Saved' });
    renderToasts();
    const toast = screen.getByRole('status');

    act(() => {
      vi.advanceTimersByTime(4000);
    });
    fireEvent.mouseEnter(toast);

    act(() => {
      vi.advanceTimersByTime(30000);
    });
    expect(screen.getByText('Saved')).toBeInTheDocument();

    fireEvent.mouseLeave(toast);
    act(() => {
      vi.advanceTimersByTime(1999);
    });
    expect(screen.getByText('Saved')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('pauses while a notification action has focus and resumes after it leaves', () => {
    pushNotification({
      kind: 'success',
      title: 'Saved',
      actions: [{ label: 'Undo', onClick: () => undefined }],
    });
    renderToasts();
    const action = screen.getByRole('button', { name: 'Undo' });

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    act(() => action.focus());

    act(() => {
      vi.advanceTimersByTime(30000);
    });
    expect(screen.getByText('Saved')).toBeInTheDocument();

    act(() => action.blur());
    act(() => {
      vi.advanceTimersByTime(2999);
    });
    expect(screen.getByText('Saved')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('starts the six seconds when a progress notification becomes a success', () => {
    const id = pushNotification({
      kind: 'progress',
      title: 'Downloading OptiScaler',
      progress: 64,
    });
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(30000);
    });
    expect(screen.getByText('Downloading OptiScaler')).toBeInTheDocument();

    act(() => {
      updateNotification(id, {
        kind: 'success',
        title: 'OptiScaler installed',
        progress: undefined,
      });
    });

    act(() => {
      vi.advanceTimersByTime(5999);
    });
    expect(screen.getByText('OptiScaler installed')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('does not restart the timer when an unrelated update re-renders the card', () => {
    const id = pushNotification({ kind: 'success', title: 'Saved' });
    renderToasts();

    act(() => {
      vi.advanceTimersByTime(3000);
    });
    act(() => {
      updateNotification(id, { title: 'Saved just now' });
    });

    act(() => {
      vi.advanceTimersByTime(2999);
    });
    expect(screen.getByText('Saved just now')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(uiStore.state.toasts).toHaveLength(0);
  });

  it('runs an action once and keeps it keyboard reachable', () => {
    const onClick = vi.fn();
    pushNotification({
      kind: 'success',
      title: 'Saved',
      persistent: true,
      actions: [{ label: 'Undo', onClick }],
    });
    renderToasts();

    const action = screen.getByRole('button', { name: 'Undo' });
    expect(action).toHaveAttribute('type', 'button');
    fireEvent.click(action);

    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('cleans its timer up on unmount', () => {
    pushNotification({ kind: 'success', title: 'Saved' });
    const view = renderToasts();

    view.unmount();
    act(() => {
      vi.advanceTimersByTime(60000);
    });

    expect(uiStore.state.toasts).toHaveLength(1);
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
