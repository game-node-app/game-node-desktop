import { useEffect, useId, useRef, useSyncExternalStore } from 'react';

/**
 * Shared stacking for every modal surface. The last modal opened sits on top,
 * regardless of where it is mounted in the React tree. Modal implementations
 * pass the returned `zIndex` to their portal/root and leave Escape handling to
 * this module so only the topmost surface closes.
 */
export const MODAL_LAYER_BASE_Z_INDEX = 1000;

export interface ModalLayerEntry {
  id: string;
  order: number;
  closeOnEscape: boolean;
  onClose: () => void;
  baseZIndex: number;
  zIndex: number;
}

let nextOrder = 0;
let entries: ModalLayerEntry[] = [];
let snapshot: ModalLayerEntry[] = [];
const listeners = new Set<() => void>();
let listeningForEscape = false;

function emit(): void {
  snapshot = entries;
  listeners.forEach((listener) => listener());
}

function recompute(): void {
  const sorted = [...entries].sort((a, b) => a.order - b.order);
  const highestBase = sorted.reduce(
    (highest, entry) => Math.max(highest, entry.baseZIndex),
    MODAL_LAYER_BASE_Z_INDEX,
  );
  entries = sorted.map((entry, index) => ({
    ...entry,
    zIndex: highestBase + index,
  }));
  emit();
}

function handleEscape(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || event.defaultPrevented || event.isComposing) {
    return;
  }
  const top = entries[entries.length - 1];
  if (!top || !top.closeOnEscape) return;
  event.preventDefault();
  top.onClose();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ModalLayerEntry[] {
  return snapshot;
}

function ensureEscapeListener(): void {
  if (listeningForEscape || typeof window === 'undefined') return;
  window.addEventListener('keydown', handleEscape);
  listeningForEscape = true;
}

function removeEscapeListener(): void {
  if (!listeningForEscape || typeof window === 'undefined') return;
  window.removeEventListener('keydown', handleEscape);
  listeningForEscape = false;
}

function registerLayer(
  id: string,
  closeOnEscape: boolean,
  onClose: () => void,
  baseZIndex: number,
): () => void {
  ensureEscapeListener();
  entries = [
    ...entries,
    {
      id,
      order: ++nextOrder,
      closeOnEscape,
      onClose,
      baseZIndex,
      zIndex: MODAL_LAYER_BASE_Z_INDEX,
    },
  ];
  recompute();

  return () => {
    entries = entries.filter((entry) => entry.id !== id);
    recompute();
    if (entries.length === 0) removeEscapeListener();
  };
}

function updateLayer(
  id: string,
  closeOnEscape: boolean,
  baseZIndex: number,
): void {
  let changed = false;
  entries = entries.map((entry) => {
    if (entry.id !== id) return entry;
    if (
      entry.closeOnEscape === closeOnEscape &&
      entry.baseZIndex === baseZIndex
    ) {
      return entry;
    }
    changed = true;
    return { ...entry, closeOnEscape, baseZIndex };
  });
  if (changed) recompute();
}

export interface UseModalLayerOptions {
  opened: boolean;
  closeOnEscape: boolean;
  onClose: () => void;
  zIndex?: number;
}

export interface ModalLayerState {
  zIndex: number;
  isTopmost: boolean;
}

export function useModalLayer({
  opened,
  closeOnEscape,
  onClose,
  zIndex = MODAL_LAYER_BASE_Z_INDEX,
}: UseModalLayerOptions): ModalLayerState {
  const id = useId();
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!opened) return;
    return registerLayer(id, closeOnEscape, () => onCloseRef.current(), zIndex);
    // `closeOnEscape` and `zIndex` changes are applied by the effect below
    // without reordering an already open surface.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened, id]);

  useEffect(() => {
    if (!opened) return;
    updateLayer(id, closeOnEscape, zIndex);
  }, [opened, id, closeOnEscape, zIndex]);

  const layers = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  const entry = layers.find((layer) => layer.id === id);

  return {
    zIndex: entry?.zIndex ?? MODAL_LAYER_BASE_Z_INDEX,
    isTopmost:
      entry != null &&
      layers.length > 0 &&
      layers[layers.length - 1].id === id,
  };
}
