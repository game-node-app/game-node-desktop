import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import {
  formatSessionTime,
  sessionElapsedSeconds,
  useSessionClock,
} from './useSessionClock';

const NOW = 1_760_000_000;

describe('session clock', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('formats h:mm:ss with padded minutes and seconds', () => {
    expect(formatSessionTime(0)).toBe('0:00:00');
    expect(formatSessionTime(2530)).toBe('0:42:10');
    expect(formatSessionTime(3 * 3600 + 5 * 60 + 9)).toBe('3:05:09');
  });

  it('returns null for an unknown start instead of a fabricated zero', () => {
    expect(sessionElapsedSeconds(null, NOW)).toBeNull();
    expect(sessionElapsedSeconds(undefined, NOW)).toBeNull();
    expect(sessionElapsedSeconds(NOW - 90, NOW)).toBe(90);
  });

  it('ticks every second from the supplied start and stops when not running', () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW * 1000);

    const running = renderHook(() => useSessionClock(NOW - 2530));
    expect(running.result.current).toBe(2530);

    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(running.result.current).toBe(2532);

    const idle = renderHook(() => useSessionClock(null));
    expect(idle.result.current).toBeNull();
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(idle.result.current).toBeNull();
  });
});
