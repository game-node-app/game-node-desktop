import { useEffect, useRef } from 'react';
import { useLocalGames } from '../../api';
import { pushToast } from '../../store/uiStore';

/**
 * Shows the local-games error as a danger toast, once per distinct message.
 *
 * A failed load (for example, a saved record the engine cannot read) empties the DRM-free
 * views. Without this toast the user sees empty lists and no reason why.
 */
export function useLocalGameErrorToast(): void {
  const { error } = useLocalGames();
  const lastShown = useRef<string | null>(null);

  useEffect(() => {
    const message = error ?? null;
    if (message && message !== lastShown.current) {
      pushToast('danger', message);
    }
    lastShown.current = message;
  }, [error]);
}
