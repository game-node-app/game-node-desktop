import { useEffect, useRef, useState } from 'react';

/** `0:42:10`, with hours unpadded and minutes/seconds padded. */
export function formatSessionTime(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = seconds % 60;
  return `${hours}:${String(minutes).padStart(2, '0')}:${String(rest).padStart(2, '0')}`;
}

/**
 * Elapsed seconds of an execution that began at `startedAt` (Unix seconds).
 * `null` means the start is unknown; it is never presented as a zero clock.
 */
export function sessionElapsedSeconds(
  startedAt: number | null | undefined,
  nowSeconds: number,
): number | null {
  if (startedAt == null) return null;
  return Math.max(0, Math.floor(nowSeconds - startedAt));
}

/**
 * One shared wall clock. The interval only runs while `active`, so an idle
 * surface does not wake up every second.
 */
export function useNowSeconds(
  intervalMs = 1000,
  active = true,
  initialNow?: number,
): number {
  const [now, setNow] = useState(
    () => initialNow ?? Math.floor(Date.now() / 1000),
  );
  const pinnedInitial = useRef(initialNow);

  useEffect(() => {
    if (!active) return;
    // Catch up after the surface becomes active again. An explicit initial
    // value stays authoritative until the first interval tick, so tests and
    // embedded previews can pin the clock.
    if (pinnedInitial.current == null) {
      setNow(Math.floor(Date.now() / 1000));
    }
    const timer = window.setInterval(
      () => setNow(Math.floor(Date.now() / 1000)),
      intervalMs,
    );
    return () => window.clearInterval(timer);
  }, [active, intervalMs]);

  return now;
}

/**
 * Live session seconds for the selected running game. Mount one per detail
 * pane, not one per library item: views with many rows should read a single
 * {@link useNowSeconds} clock and pass rows through {@link sessionElapsedSeconds}.
 */
export function useSessionClock(
  startedAt: number | null | undefined,
): number | null {
  const now = useNowSeconds(1000, startedAt != null);
  return sessionElapsedSeconds(startedAt, now);
}
