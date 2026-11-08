import { Store, useStore } from '@tanstack/react-store';

/**
 * State of the guided setup surfaces.
 *
 * The setup modal is a singleton: `openPortSetup` selects the entry and increments
 * `session`, which remounts the modal with fresh per-install form state. The job itself keeps
 * running in the host after the modal closes, so nothing here tracks progress.
 */
export interface PortSetupState {
  open: boolean;
  /** Catalog entry the modal configures; null when no modal was opened yet. */
  catalogId: string | null;
  /** Set when the user retries a failed, cancelled or interrupted job. */
  retryJobId: string | null;
  /** Increments on every open so a second open starts from a clean form. */
  session: number;
}

export const initialPortSetupState: PortSetupState = {
  open: false,
  catalogId: null,
  retryJobId: null,
  session: 0,
};

export const portSetupStore = new Store<PortSetupState>(initialPortSetupState);

export interface OpenPortSetupOptions {
  /** Reuse the stored install plan of this job (Retry from a result). */
  retryJobId?: string;
}

/**
 * Opens the guided setup modal for one catalog entry. Release-only entries use the same
 * modal with just the release and parent-folder steps.
 */
export function openPortSetup(
  catalogId: string,
  options: OpenPortSetupOptions = {},
): void {
  if (!catalogId) return;
  portSetupStore.setState((previous) => ({
    open: true,
    catalogId,
    retryJobId: options.retryJobId ?? null,
    session: previous.session + 1,
  }));
}

/** Closes the modal without touching the running job. */
export function closePortSetup(): void {
  portSetupStore.setState((previous) =>
    previous.open ? { ...previous, open: false } : previous,
  );
}

/** Restores the initial state. Used by tests and when the destination unmounts. */
export function resetPortSetup(): void {
  portSetupStore.setState(() => ({ ...initialPortSetupState }));
}

export function usePortSetupStore<TSelected = PortSetupState>(
  selector?: (state: PortSetupState) => TSelected,
): TSelected {
  return useStore(portSetupStore, selector ?? ((state) => state as unknown as TSelected));
}

/** A guided-setup session id used as the `jobId` of `ports:progress` while hashing a pick. */
export function createSetupId(): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2, 10);
  return `setup-${random}`;
}
