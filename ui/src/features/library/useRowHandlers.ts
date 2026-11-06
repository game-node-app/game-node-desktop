import type { MouseEvent as ReactMouseEvent } from 'react';
import { useCallback, useEffect, useMemo, useRef } from 'react';
import { commands, useLibraryView } from '../../api';
import { openContextMenu } from '../../store/uiStore';

export interface RowHandlers {
  onClick: (e: ReactMouseEvent, key: string) => void;
  onContextMenu: (e: ReactMouseEvent, key: string) => void;
  onDoubleClick: (e: ReactMouseEvent, key: string) => void;
}

/**
 * Selection state plus referentially stable pointer handlers shared by the grid
 * and the list, so memoized tiles/rows are not re-rendered by unrelated updates.
 */
export function useLibrarySelection(): {
  selectedKeys: ReadonlySet<string>;
  handlers: RowHandlers;
} {
  const library = useLibraryView();
  const selected = library?.selected;
  const selectedKeys = useMemo(() => new Set(selected ?? []), [selected]);

  const selectedRef = useRef(selectedKeys);
  useEffect(() => {
    selectedRef.current = selectedKeys;
  }, [selectedKeys]);

  const onClick = useCallback((e: ReactMouseEvent, key: string) => {
    if (e.button !== 0) return;
    if (e.shiftKey) {
      commands.libraryInput({ select: { key, mode: 'range' } });
    } else if (e.ctrlKey || e.metaKey) {
      commands.libraryInput({ select: { key, mode: 'toggle' } });
    } else {
      commands.libraryInput({ select: { key, mode: 'replace' } });
    }
  }, []);

  const onContextMenu = useCallback((e: ReactMouseEvent, key: string) => {
    e.preventDefault();
    if (!selectedRef.current.has(key)) {
      commands.libraryInput({ select: { key, mode: 'replace' } });
    }
    openContextMenu(e.clientX, e.clientY, key);
  }, []);

  const onDoubleClick = useCallback(() => {
    commands.activateEntry();
  }, []);

  const handlers = useMemo(
    () => ({ onClick, onContextMenu, onDoubleClick }),
    [onClick, onContextMenu, onDoubleClick],
  );

  return { selectedKeys, handlers };
}
