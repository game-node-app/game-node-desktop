export interface ModalSize {
  width: number;
  height: number;
}

export interface ModalGeometry extends ModalSize {
  x: number;
  y: number;
}

export interface ModalViewport {
  width: number;
  height: number;
}

export interface GeometryConstraints {
  defaultSize: ModalSize;
  minSize: ModalSize;
  maxSize?: Partial<ModalSize>;
  offset: number;
  viewport?: ModalViewport;
}

interface StoredGeometry {
  v: 1;
  x: number;
  y: number;
  width: number;
  height: number;
}

export const FLOATING_MODAL_STORAGE_PREFIX = 'gamenode.floating-modal.';

function currentViewport(): ModalViewport {
  if (typeof window === 'undefined') {
    return { width: 1024, height: 768 };
  }
  return { width: window.innerWidth, height: window.innerHeight };
}

function finite(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

function availableSize(
  viewport: ModalViewport,
  offset: number,
): ModalSize {
  return {
    width: Math.max(1, viewport.width - offset * 2),
    height: Math.max(1, viewport.height - offset * 2),
  };
}

/**
 * Fits a requested size inside the viewport, respecting the caller's minimum
 * and maximum sizes. When the viewport is smaller than the configured minimum,
 * the viewport wins so the window never starts off-screen.
 */
export function fitSize(
  size: ModalSize,
  constraints: GeometryConstraints,
): ModalSize {
  const viewport = constraints.viewport ?? currentViewport();
  const available = availableSize(viewport, constraints.offset);
  const maxWidth = Math.min(
    constraints.maxSize?.width ?? Number.POSITIVE_INFINITY,
    available.width,
  );
  const maxHeight = Math.min(
    constraints.maxSize?.height ?? Number.POSITIVE_INFINITY,
    available.height,
  );
  const minWidth = Math.min(constraints.minSize.width, maxWidth);
  const minHeight = Math.min(constraints.minSize.height, maxHeight);

  return {
    width: Math.round(clamp(size.width, minWidth, maxWidth)),
    height: Math.round(clamp(size.height, minHeight, maxHeight)),
  };
}

export function clampGeometry(
  geometry: ModalGeometry,
  constraints: GeometryConstraints,
): ModalGeometry {
  const viewport = constraints.viewport ?? currentViewport();
  const size = fitSize(geometry, constraints);
  const maxX = viewport.width - size.width - constraints.offset;
  const maxY = viewport.height - size.height - constraints.offset;

  return {
    ...size,
    x: Math.round(clamp(geometry.x, constraints.offset, maxX)),
    y: Math.round(clamp(geometry.y, constraints.offset, maxY)),
  };
}

export function createCenteredGeometry(
  constraints: GeometryConstraints,
): ModalGeometry {
  const viewport = constraints.viewport ?? currentViewport();
  const size = fitSize(constraints.defaultSize, constraints);

  return {
    ...size,
    x: Math.round((viewport.width - size.width) / 2),
    y: Math.round((viewport.height - size.height) / 2),
  };
}

function storageKey(geometryKey: string): string {
  return `${FLOATING_MODAL_STORAGE_PREFIX}${geometryKey}`;
}

function isStoredGeometry(value: unknown): value is StoredGeometry {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<StoredGeometry>;
  return (
    candidate.v === 1 &&
    finite(candidate.x) &&
    finite(candidate.y) &&
    finite(candidate.width) &&
    finite(candidate.height) &&
    candidate.width > 0 &&
    candidate.height > 0
  );
}

export function readStoredGeometry(
  geometryKey: string,
  constraints: GeometryConstraints,
): ModalGeometry | null {
  if (typeof window === 'undefined') return null;
  try {
    const raw = window.localStorage.getItem(storageKey(geometryKey));
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!isStoredGeometry(parsed)) return null;
    return clampGeometry(parsed, constraints);
  } catch {
    return null;
  }
}

export function writeStoredGeometry(
  geometryKey: string,
  geometry: ModalGeometry,
): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(
      storageKey(geometryKey),
      JSON.stringify({ v: 1, ...geometry }),
    );
  } catch {
    // Storage may be unavailable or full; floating still works for this session.
  }
}

export function clearStoredGeometry(geometryKey: string): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.removeItem(storageKey(geometryKey));
  } catch {
    // Ignore unavailable storage.
  }
}
