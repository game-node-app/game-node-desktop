import { describe, expect, it, vi } from 'vitest';
import { fireEvent, renderHook, waitFor } from '@testing-library/react';
import { useModalLayer } from './modalLayer';

describe('modalLayer', () => {
  it('renders the last opened layer on top and closes only it on Escape', async () => {
    const first = vi.fn();
    const second = vi.fn();
    const firstLayer = renderHook(() =>
      useModalLayer({ opened: true, closeOnEscape: true, onClose: first }),
    );
    const secondLayer = renderHook(() =>
      useModalLayer({ opened: true, closeOnEscape: true, onClose: second }),
    );

    await waitFor(() => {
      expect(secondLayer.result.current.zIndex).toBeGreaterThan(
        firstLayer.result.current.zIndex,
      );
    });

    fireEvent.keyDown(window, { key: 'Escape' });

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
