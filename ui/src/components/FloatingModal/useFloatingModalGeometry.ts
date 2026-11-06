import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from 'react';
import type { SetFloatingWindowPosition } from '@mantine/hooks';
import {
  clampGeometry,
  createCenteredGeometry,
  readStoredGeometry,
  writeStoredGeometry,
  type GeometryConstraints,
  type ModalGeometry,
} from './geometry';

export interface UseFloatingModalGeometryOptions {
  geometryKey?: string;
  persist: boolean;
  defaultSize: GeometryConstraints['defaultSize'];
  minSize: GeometryConstraints['minSize'];
  maxSize?: GeometryConstraints['maxSize'];
  offset: number;
}

export interface FloatingModalGeometry {
  rootRef: RefObject<HTMLDivElement | null>;
  setPositionRef: RefObject<SetFloatingWindowPosition | null>;
  initialPosition: { left: number; top: number };
  dimensions: {
    initialWidth: number;
    initialHeight: number;
    minWidth: number;
    minHeight: number;
    maxWidth?: number;
    maxHeight?: number;
  };
  onPositionChange: (position: { x: number; y: number }) => void;
  onDragEnd: () => void;
  onSizeChange: (size: { width: number; height: number }) => void;
  onResizeEnd: () => void;
}

const RESIZE_PERSIST_DELAY_MS = 150;

export function useFloatingModalGeometry(
  options: UseFloatingModalGeometryOptions,
): FloatingModalGeometry {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const [initialGeometry] = useState<ModalGeometry>(() => {
    const constraints: GeometryConstraints = {
      defaultSize: options.defaultSize,
      minSize: options.minSize,
      maxSize: options.maxSize,
      offset: options.offset,
    };
    const stored =
      options.persist && options.geometryKey
        ? readStoredGeometry(options.geometryKey, constraints)
        : null;
    return stored ?? createCenteredGeometry(constraints);
  });

  const positionRef = useRef({ x: initialGeometry.x, y: initialGeometry.y });
  const sizeRef = useRef({
    width: initialGeometry.width,
    height: initialGeometry.height,
  });
  const rootRef = useRef<HTMLDivElement | null>(null);
  const setPositionRef = useRef<SetFloatingWindowPosition | null>(null);
  const persistTimerRef = useRef<number | null>(null);

  const persistNow = useCallback(() => {
    const { geometryKey, persist, ...rest } = optionsRef.current;
    if (!persist || !geometryKey) return;

    const constraints: GeometryConstraints = {
      defaultSize: rest.defaultSize,
      minSize: rest.minSize,
      maxSize: rest.maxSize,
      offset: rest.offset,
    };
    const geometry = clampGeometry(
      {
        ...positionRef.current,
        ...sizeRef.current,
      },
      constraints,
    );
    writeStoredGeometry(geometryKey, geometry);
  }, []);

  const schedulePersist = useCallback(() => {
    if (typeof window === 'undefined') return;
    if (persistTimerRef.current != null) {
      window.clearTimeout(persistTimerRef.current);
    }
    persistTimerRef.current = window.setTimeout(() => {
      persistTimerRef.current = null;
      persistNow();
    }, RESIZE_PERSIST_DELAY_MS);
  }, [persistNow]);

  useEffect(() => {
    if (typeof window === 'undefined') return;

    const handleWindowResize = () => {
      const current = optionsRef.current;
      const constraints: GeometryConstraints = {
        defaultSize: current.defaultSize,
        minSize: current.minSize,
        maxSize: current.maxSize,
        offset: current.offset,
      };
      const next = clampGeometry(
        {
          ...positionRef.current,
          ...sizeRef.current,
        },
        constraints,
      );

      positionRef.current = { x: next.x, y: next.y };
      sizeRef.current = { width: next.width, height: next.height };

      const element = rootRef.current;
      if (element) {
        element.style.width = `${next.width}px`;
        element.style.height = `${next.height}px`;
      }
      setPositionRef.current?.({ left: next.x, top: next.y });
      schedulePersist();
    };

    window.addEventListener('resize', handleWindowResize);
    return () => {
      window.removeEventListener('resize', handleWindowResize);
      if (persistTimerRef.current != null) {
        window.clearTimeout(persistTimerRef.current);
        persistTimerRef.current = null;
      }
      persistNow();
    };
  }, [persistNow, schedulePersist]);

  const initialPosition = useMemo(
    () => ({ left: initialGeometry.x, top: initialGeometry.y }),
    [initialGeometry.x, initialGeometry.y],
  );

  const dimensions = useMemo(
    () => ({
      initialWidth: initialGeometry.width,
      initialHeight: initialGeometry.height,
      minWidth: options.minSize.width,
      minHeight: options.minSize.height,
      maxWidth: options.maxSize?.width,
      maxHeight: options.maxSize?.height,
    }),
    [
      initialGeometry.width,
      initialGeometry.height,
      options.minSize.width,
      options.minSize.height,
      options.maxSize?.width,
      options.maxSize?.height,
    ],
  );

  const onPositionChange = useCallback((position: { x: number; y: number }) => {
    positionRef.current = position;
  }, []);

  const onSizeChange = useCallback((size: { width: number; height: number }) => {
    sizeRef.current = size;
  }, []);

  return {
    rootRef,
    setPositionRef,
    initialPosition,
    dimensions,
    onPositionChange,
    onDragEnd: persistNow,
    onSizeChange,
    onResizeEnd: persistNow,
  };
}
