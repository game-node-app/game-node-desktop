/// <reference types="vite/client" />
/**
 * PC ports contract.
 *
 * This module is the single source of truth for the shape of the PC ports feature on the
 * UI side. The Rust host is implemented against these declarations:
 *
 * - `Snapshot.ports` (`PortsView`) carries the catalog status, the merged entries, the
 *   install provenance, the install jobs and the unseen badge count. The host rebuilds it
 *   on `engine:snapshot` only when something material changes, never for every progress
 *   tick (D5).
 * - `ports:progress` (`PortProgress`) is a separate event, coalesced by the host to at most
 *   four updates per second. Consumers subscribe with `usePortProgress(jobId)`, whose
 *   react-query key only re-renders that one consumer.
 * - The ten `commands.ports*` commands below are the only invokes the feature uses. Their
 *   Tauri argument names are documented on each signature.
 *
 * JSON is camelCase on both sides, matching the rest of `api/types.ts`.
 */
import { useEffect, useMemo } from 'react';
import {
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from '@tanstack/react-query';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands } from './commands';
import { getMockHost, mockEnabled } from './mock';
import { useSnapshot } from './useSnapshot';
import { assetProtocolBase, type Snapshot } from './types';

// ---------------------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------------------

/** PC port installs are Windows-only in this release (desktop-shell spec). */
export type PortPlatform = 'windows' | 'linux' | 'macos';

/**
 * Resolves the platform used to show or hide the PC ports destination. A host-reported
 * platform wins over the user agent, which lets the mock present a Windows build on another
 * OS. Older hosts do not send `AppInfo.platform`.
 */
export function detectPortPlatform(
  userAgent?: string | null,
  reported?: string | null,
): PortPlatform {
  if (reported === 'windows' || reported === 'linux' || reported === 'macos') {
    return reported;
  }
  const ua = userAgent ?? '';
  if (ua.includes('Windows')) return 'windows';
  if (ua.includes('Macintosh') || ua.includes('Mac OS X')) return 'macos';
  return 'linux';
}

// ---------------------------------------------------------------------------------------
// Catalog status
// ---------------------------------------------------------------------------------------

/**
 * - `idle`: PC ports was never opened, so no fetch has run.
 * - `refreshing`: a fetch is running; `entries` may still hold the previous snapshot.
 * - `ready`: the last refresh completed and the snapshot is current.
 * - `stale`: a snapshot exists, but the last refresh failed or it is older than a day.
 * - `unavailable`: no usable snapshot exists and the last fetch failed.
 */
export type PortCatalogState =
  | 'idle'
  | 'refreshing'
  | 'ready'
  | 'unavailable'
  | 'stale';

/** Why the last catalog refresh failed. The UI localises these codes. */
export type PortCatalogError =
  | 'unreachable'
  | 'rateLimited'
  | 'invalidResponse'
  | 'emptyListing'
  | 'unknown';

export interface PortCatalogStatus {
  state: PortCatalogState;
  /** Unix seconds of the last complete stored snapshot; null before the first success. */
  fetchedAt: number | null;
  /** Entries in the stored snapshot, before search or filters. */
  entryCount: number;
  /** Set while `stale` or `unavailable`; null after a successful refresh. */
  error?: PortCatalogError | null;
}

// ---------------------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------------------

/** Whether the artwork cache holds each size (D9). Tiles fall back to a titled placeholder. */
export interface PortArtView {
  capsule: boolean;
  hero: boolean;
}

export type PortArtKind = 'capsule' | 'hero';

/** A text-only named link. Quiver logos are never used (desktop-port-catalog spec). */
export interface PortLink {
  label: string;
  url: string;
}

export type PortOperatingSystem = 'windows' | 'linux' | 'macos';

/** Why an entry that supports Windows offers no installable Windows asset. */
export type PortNoAssetReason =
  | 'installerOnly'
  | 'noWindowsAsset'
  | 'missingAsset'
  | 'noExecutable';

/** Why Install is unavailable for an entry. */
export type PortInstallBlockReason =
  | 'withdrawn'
  | 'noVerifiedRelease'
  | PortNoAssetReason;

export type PortSetupKind = 'file' | 'folder';

/** Where a supplied file or folder is placed. */
export type PortPlacementRoot = 'game' | 'data' | 'config' | 'localData';

export type PortArgumentKind = 'flag' | 'positional' | 'env';

/**
 * How a supplied selection reaches the port when it is not copied. `flag` arguments are
 * appended as `name path`, `positional` ones as a bare path, and `env` entries become an
 * environment variable applied by the DRM-free launcher.
 */
export interface PortArgumentSpec {
  kind: PortArgumentKind;
  /** Flag name including dashes (`--iso`) or the environment variable name. */
  name: string;
}

/** A pinned console-package extractor (D8) offered as an alternative to a folder. */
export interface PortPackageOption {
  /** Overlay extractor id, e.g. `ps4-pkg`. */
  extractorId: string;
  /** Display name of the pinned tool. */
  toolName: string;
  /** Tool repository, for the licence and source row. */
  toolSourceUrl: string | null;
  toolLicense: string | null;
  /** Title id the packages must contain, e.g. `CUSA03173`. */
  titleId: string | null;
  /** Update version the extraction merges, e.g. `01.09`; null when no update is needed. */
  updateVersion: string | null;
  /** Whether the user must also choose an update package. */
  requiresUpdate: boolean;
}

/**
 * One file or folder the user must supply for guided setup (D4, D7). Slots arrive in
 * recipe order, with the overlay's replacements already merged.
 */
export interface PortSetupSlot {
  id: string;
  /** Short label shown on the setup row. */
  label: string;
  /** Longer instruction, when the overlay or upstream provides one. */
  prompt: string | null;
  kind: PortSetupKind;
  /** Required slots block the install until the user picks an accepted source. */
  required: boolean;
  /** Lowercase extensions with the dot for `file` slots; empty accepts any file. */
  extensions: string[];
  /** Paths that must exist inside a `folder` slot, relative to the picked folder. */
  contains: string[];
  /** Human-readable accepted sources, e.g. `CUSA03173 (01.09)`. */
  acceptedLabels: string[];
  /** Placement destination relative to `root`; null when the slot is passed as an argument. */
  destination: string | null;
  root: PortPlacementRoot | null;
  /** Set when the slot is passed to the port instead of copied. */
  argument: PortArgumentSpec | null;
  /** Set when console packages can produce this slot through a pinned extractor. */
  packageOption: PortPackageOption | null;
}

/** One merged catalog entry. Only entries supporting Windows are present. */
export interface PortEntryView {
  /** Upstream stable id; also the `port-art` path segment. */
  catalogId: string;
  slug: string;
  name: string;
  /** Original game titles, exactly as upstream names them. */
  games: string[];
  /** Source consoles, e.g. `PlayStation 4`. */
  consoles: string[];
  supportedOS: string[];
  /** The release the upstream marks verified exists. */
  verified: boolean;
  /** Only pre-release assets exist; Install needs a verified release. */
  prerelease: boolean;
  /** Release an install would use, as known from the listing; null when none is known yet. */
  version: string | null;
  /** Whether the merged recipe has at least one setup slot. */
  guided: boolean;
  /** Withdrawn upstream; never installable. */
  withdrawn: boolean;
  hasWindowsAsset: boolean;
  noAssetReason: PortNoAssetReason | null;
  /** Whether Install can start; the detail pane shows `installBlockReason` when false. */
  installable: boolean;
  installBlockReason: PortInstallBlockReason | null;
  art: PortArtView;
  /** The port project (repository or homepage). */
  project: PortLink | null;
  /** The original game or source project, when distinct from `project`. */
  source: PortLink | null;
  /** Licence recorded by the overlay; the upstream listing has no licence field. */
  license: string | null;
  /** Setup slots in recipe order. Empty for a release-only entry. */
  requirements: PortSetupSlot[];
  /** Registration key of the library entry this install produced, when installed. */
  installedRegistrationKey: string | null;
}

// ---------------------------------------------------------------------------------------
// Installations and jobs
// ---------------------------------------------------------------------------------------

/** Provenance for a completed install (D10), keyed by registration key in `PortsView`. */
export interface PortInstallationView {
  /** DRM-free registration key, e.g. `drmfree:<id>`. */
  registrationKey: string;
  catalogId: string;
  slug: string;
  portName: string;
  installedVersion: string;
  installFolder: string;
  jobId: string | null;
  installedAt: number | null;
}

/**
 * Job phases from D5. Terminal phases are `completed`, `cancelled`, `failed` and
 * `interrupted`; `cancelling` is transient.
 */
export type PortJobPhase =
  | 'queued'
  | 'resolving'
  | 'downloading'
  | 'verifying'
  | 'extracting'
  | 'suppliedFiles'
  | 'finalizing'
  | 'registering'
  | 'completed'
  | 'cancelling'
  | 'cancelled'
  | 'failed'
  | 'interrupted';

export type PortJobOutcome = 'completed' | 'cancelled' | 'failed' | 'interrupted';

export interface PortJobFailure {
  /** Diagnostic message shown to the user; the host owns its wording. */
  message: string;
  /** Capped stdout/stderr of the pinned extractor, for the failure details view. */
  toolOutput?: string | null;
}

export interface PortJobView {
  jobId: string;
  catalogId: string;
  slug: string;
  name: string;
  phase: PortJobPhase;
  /** The supplied slot being processed while `phase` is `suppliedFiles`. */
  slotId?: string | null;
  /** Terminal outcome; null while queued, running or cancelling. */
  outcome?: PortJobOutcome | null;
  failure?: PortJobFailure | null;
  /** Whether the current phase can be cancelled. */
  interruptible: boolean;
  /** Whether a failed, cancelled or interrupted job offers Retry. */
  retryable: boolean;
  /**
   * True on a completed job whose preferred executable was missing; the user picks the
   * executable inside the install folder before the entry can launch.
   */
  needsExecutable?: boolean | null;
  /**
   * False until the affected entry's result is opened. The sidebar badge counts running
   * jobs plus completed, failed and interrupted jobs that are still unseen.
   */
  seen: boolean;
  startedAt: number | null;
  finishedAt?: number | null;
}

// ---------------------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------------------

export type PortViewMode = 'grid' | 'list';

/** Catalog filters; they combine with AND. */
export interface PortFilters {
  /** Console names from `PortEntryView.consoles`; empty means every console. */
  consoles: string[];
  /** Only entries with guided setup. */
  guided: boolean;
  /** Only entries with a verified release. */
  verified: boolean;
  /** Only entries installed by GameNode. */
  installed: boolean;
}

/** Remembered view state (desktop-port-catalog spec). */
export interface PortPreferences {
  view: PortViewMode;
  filters: PortFilters;
  /** Offered first by the parent-folder picker; null before the first install. */
  lastParentFolder: string | null;
}

/** Patch accepted by `commands.portsSetPreferences`; only present fields change. */
export interface PortPreferencesPatch {
  view?: PortViewMode;
  filters?: Partial<PortFilters>;
  lastParentFolder?: string | null;
}

// ---------------------------------------------------------------------------------------
// Snapshot payload
// ---------------------------------------------------------------------------------------

/**
 * `Snapshot.ports`. One payload serves the sidebar badge, the catalog and the detail pane.
 * `entries` and `provenance` are already merged with the GameNode overlay and filtered to
 * Windows entries; the UI does not see hidden or Linux-only entries.
 */
export interface PortsView {
  catalog: PortCatalogStatus;
  entries: PortEntryView[];
  /** Provenance by registration key (`drmfree:<id>`). */
  provenance: Record<string, PortInstallationView>;
  /** Newest jobs first. Finished jobs stay until the user starts another install. */
  jobs: PortJobView[];
  /**
   * Running jobs plus completed, failed or interrupted jobs the user has not opened yet.
   * The sidebar badge shows this count.
   */
  unseenCount: number;
  preferences: PortPreferences;
}

// ---------------------------------------------------------------------------------------
// Detail pane payload (lazy, D3)
// ---------------------------------------------------------------------------------------

export type PortAssetFormat = 'zip' | '7z' | 'exe';

/** One release with the Windows asset selection and its safety flags. */
export interface PortReleaseView {
  version: string;
  /** ISO-8601 date as upstream sends it; null when unknown. */
  releasedAt: string | null;
  verified: boolean;
  withdrawn: boolean;
  /** Windows asset chosen by the recipe's filter; null when none exists. */
  assetName: string | null;
  assetFormat: PortAssetFormat | null;
  sizeBytes: number | null;
  /** The asset has a verified SHA-256 checksum. */
  checksumVerified: boolean;
  /** Only an unverified checksum exists; Install asks for explicit confirmation. */
  requiresChecksumConfirmation: boolean;
  /** Malware-scan warning text; Install asks for a confirmation naming the risk. */
  malwareFlag: string | null;
}

/**
 * Returned by `commands.portsDetail`. The host caches it for 15 minutes and refetches when
 * the detail pane opens without a fresh copy.
 */
export interface PortDetailView {
  catalogId: string;
  description: string | null;
  developer: string | null;
  license: string | null;
  project: PortLink | null;
  source: PortLink | null;
  art: PortArtView;
  requirements: PortSetupSlot[];
  /** Newest releases first. */
  releases: PortReleaseView[];
  /** The release an install would use: the verified one or the overlay's pin. */
  selectedRelease: PortReleaseView | null;
  /** Explains a withdrawal when the selected release is withdrawn. */
  withdrawalReason: string | null;
  /** Newest verified release newer than the installed version, for the update notice. */
  updateAvailable: PortReleaseView | null;
  noAssetReason: PortNoAssetReason | null;
}

// ---------------------------------------------------------------------------------------
// Invoke payloads
// ---------------------------------------------------------------------------------------

/** `ports_detail` argument: a single `catalogId` string. */
export interface PortDetailRequest {
  catalogId: string;
}

/** `ports_pick_parent` argument: an optional folder to open the native picker in. */
export interface PortPickParentRequest {
  initialFolder?: string | null;
}

/**
 * `ports_pick_supplied` request. `setupId` is a UI-generated guided-setup session id; the
 * host sends hashing progress for it on `ports:progress` with the same value in `jobId`.
 */
export interface PortPickSuppliedRequest {
  setupId: string;
  /**
   * The catalog entry whose recipe defines the slot. The host validates the pick against it; an
   * older caller that omits it only gets the picker filters.
   */
  catalogId?: string | null;
  slotId: string;
  /** `package` opens a file picker for a console package (base or update). */
  kind: 'file' | 'folder' | 'package';
  /** For `package`: true when choosing the update package instead of the base package. */
  update?: boolean;
  /** Extensions offered by the picker; the host also enforces the slot's own list. */
  extensions?: string[];
}

export type PortSuppliedStatus =
  | 'accepted'
  | {
      rejected: {
        /** Message naming the accepted title id and versions. */
        message: string;
        acceptedLabels: string[];
      };
    };

/** `ports_pick_supplied` result; null when the user cancelled the picker. */
export interface PortSuppliedPick {
  slotId: string;
  path: string;
  status: PortSuppliedStatus;
}

/** One accepted selection sent with an install request. */
export interface PortSuppliedChoice {
  slotId: string;
  path: string;
  /** True when the path is a base package for a package-capable slot. */
  package?: boolean;
  /** True when the package is the update package. */
  update?: boolean;
}

/** `ports_install` request. */
export interface PortInstallRequest {
  catalogId: string;
  /** Parent folder chosen by the user; the host creates a subfolder inside it. */
  parentFolder: string;
  supplied: PortSuppliedChoice[];
  /** Confirm an asset that only has an unverified checksum. */
  acceptUnverifiedChecksum?: boolean;
  /** Confirm an asset a malware scan flagged. */
  acceptMalwareFlag?: boolean;
  /**
   * Set by Retry to reuse the stored plan of a failed, cancelled or interrupted job. Older
   * hosts ignore it and resolve the entry again; verified downloads are still reused (D5).
   */
  retryJobId?: string;
}

// ---------------------------------------------------------------------------------------
// Progress event (separate from engine:snapshot)
// ---------------------------------------------------------------------------------------

export const PORTS_PROGRESS_EVENT = 'ports:progress';

/**
 * Phases reported on `ports:progress`. While guided setup validates a picked file or
 * folder, `jobId` is the setup session id passed to `ports_pick_supplied`.
 */
export type PortProgressPhase =
  | 'downloading'
  | 'verifying'
  | 'extracting'
  | 'hashing'
  | 'extractingPackage'
  | 'copying'
  | 'finalizing'
  | 'registering';

/** Coalesced by the host to at most four updates per second (D5). */
export interface PortProgress {
  /** Install job id, or the guided-setup session id while validating a pick. */
  jobId: string;
  phase: PortProgressPhase;
  slotId?: string | null;
  bytesDone: number;
  bytesTotal: number;
}

/** React-query key for one job's progress; ticking it never touches the snapshot query. */
export function portProgressQueryKey(jobId: string) {
  return ['ports', 'progress', jobId] as const;
}

export const PORT_DETAIL_QUERY_KEY = ['ports', 'detail'] as const;

// ---------------------------------------------------------------------------------------
// Settings: helper tools and download cache (task 8.1)
// ---------------------------------------------------------------------------------------

/** One pinned extraction tool from the overlay's `[[extractor]]` block. */
export interface PortHelperToolView {
  id: string;
  name: string;
  license: string;
  /** The GitHub repository URL, or null when the overlay names none. */
  sourceUrl: string | null;
  /** The pinned release tag. */
  version: string;
  /** Bytes held by the tool, its download and its extracted copy. */
  cacheBytes: number;
}

/**
 * Returned by `ports_tools_cache` and `ports_clear_cache`: the Settings Tools panel's PC ports
 * cache block. `helperTools` is empty while the overlay has not pinned an extractor.
 */
export interface PortToolsCacheView {
  /** Bytes held by release downloads under `<data_dir>/ports/cache`. */
  downloadsBytes: number;
  helperTools: PortHelperToolView[];
  /** True while a queued, running or executable-awaiting install would use the cache. */
  installActive: boolean;
}

/** Total bytes `Clear cache` frees: release downloads plus every pinned extraction tool. */
export function portToolsTotalBytes(view: PortToolsCacheView | null | undefined): number {
  if (!view) return 0;
  return (
    view.downloadsBytes +
    view.helperTools.reduce((total, tool) => total + tool.cacheBytes, 0)
  );
}

/** React-query key for the Tools panel's PC ports cache block. */
export const PORTS_TOOLS_CACHE_QUERY_KEY = ['ports', 'tools-cache'] as const;

export function portDetailQueryKey(catalogId: string) {
  return [...PORT_DETAIL_QUERY_KEY, catalogId] as const;
}

/**
 * Subscribes to `ports:progress`. The mock host uses the same shape through its own
 * in-memory listener set, so hooks work with and without Tauri.
 */
export async function subscribeToPortProgress(
  handler: (progress: PortProgress) => void,
): Promise<UnlistenFn> {
  if (mockEnabled) {
    return (await getMockHost()).subscribePortProgress(handler);
  }
  return listen<PortProgress>(PORTS_PROGRESS_EVENT, (event) => handler(event.payload));
}

// ---------------------------------------------------------------------------------------
// Host commands
// ---------------------------------------------------------------------------------------

/**
 * The ten PC ports invokes, grouped so a feature module can import the whole contract from
 * this file. These are the same functions as `commands.ports*`; the Tauri command name and
 * argument shape are documented on each one in `api/commands.ts`.
 */
export const portsCommands = {
  open: commands.portsOpen,
  refresh: commands.portsRefresh,
  detail: commands.portsDetail,
  pickParent: commands.portsPickParent,
  pickSupplied: commands.portsPickSupplied,
  install: commands.portsInstall,
  cancel: commands.portsCancel,
  markSeen: commands.portsMarkSeen,
  setPreferences: commands.portsSetPreferences,
  pickExecutable: commands.portsPickExecutable,
};

// ---------------------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------------------

const selectPortsView = (snapshot: Snapshot) => snapshot.ports;

/** The whole PC ports projection from the latest `engine:snapshot`. */
export function usePortsView(): PortsView {
  const { data } = useSnapshot(selectPortsView);
  return data;
}

/**
 * Progress for one install job (or guided-setup session while hashing supplied files).
 * Data lives under its own query key, so a progress tick re-renders only the components
 * that watch that job. Pass null to unsubscribe.
 */
export function usePortProgress(jobId: string | null | undefined): PortProgress | null {
  const queryClient = useQueryClient();
  const queryKey = useMemo(() => portProgressQueryKey(jobId ?? ''), [jobId]);

  useEffect(() => {
    if (!jobId) return undefined;
    let active = true;
    let unlisten: UnlistenFn | undefined;

    subscribeToPortProgress((progress) => {
      if (!active || progress.jobId !== jobId) return;
      queryClient.setQueryData<PortProgress>(queryKey, progress);
    })
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {
        // A plain browser without the event bridge has no progress to show.
      });

    return () => {
      active = false;
      unlisten?.();
    };
  }, [jobId, queryClient, queryKey]);

  const query = useQuery<PortProgress | null>({
    queryKey,
    queryFn: () => null,
    enabled: false,
    staleTime: Infinity,
    gcTime: 0,
  });

  return query.data ?? null;
}

/**
 * Lazy detail for one entry. Calls `ports_detail`; the host answers from its release cache
 * or fetches it, and caches both for 15 minutes (D3). Disabled while `catalogId` is null.
 */
export function usePortDetail(
  catalogId: string | null | undefined,
): UseQueryResult<PortDetailView | null> {
  return useQuery({
    queryKey: portDetailQueryKey(catalogId ?? ''),
    queryFn: () => commands.portsDetail(catalogId as string),
    enabled: Boolean(catalogId),
    staleTime: 15 * 60 * 1000,
  });
}

// ---------------------------------------------------------------------------------------
// Artwork
// ---------------------------------------------------------------------------------------

/**
 * URL of cached port artwork. Non-mock builds use the `port-art` route (D9); the mock uses
 * the bundled fictional covers so the browser preview has images.
 */
export function portArtUrl(catalogId: string, kind: PortArtKind = 'capsule'): string {
  if (mockEnabled) {
    let hash = 0;
    for (let index = 0; index < catalogId.length; index += 1) {
      hash = (hash * 31 + catalogId.charCodeAt(index)) | 0;
    }
    return `/mock-covers/mock-${Math.abs(hash) % 12}.svg`;
  }
  return `${assetProtocolBase()}/port-art/${encodeURIComponent(catalogId)}/${kind}`;
}
