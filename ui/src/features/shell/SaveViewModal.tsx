import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from 'react';
import { IconBookmark } from '@tabler/icons-react';
import { AppModal } from '../../components/AppModal';
import { Button, Pill, SectionLabel } from '../../components/ui';
import { commands } from '../../api/commands';
import { useAuth, useLibraryView } from '../../api/useSnapshot';
import type {
  AuthView,
  GroupBy,
  LibraryFilters,
  LibraryScope,
  LibraryView,
  SavedViewCount,
  Sort,
  SortKey,
  ViewWriteError,
  ViewWriteOperation,
} from '../../api/types';
import { closeSaveView, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatI18n } from '../library/formatters';

export interface SaveViewModalProps {
  /** Injected in tests; the store decides otherwise. */
  open?: boolean;
  /** Injected in tests; the snapshot view decides otherwise. */
  library?: LibraryView | null;
  /** Injected in tests; the store decides otherwise. */
  renameViewId?: number | null;
  /** Injected in tests; the signed-in snapshot account decides otherwise. */
  accountId?: string | null;
  onClose?: () => void;
}

/** The signed-in user id, or null while signed out, starting or switching accounts. */
function signedInAccountId(auth: AuthView): string | null {
  return typeof auth === 'object' && auth !== null && 'signedIn' in auth
    ? auth.signedIn.user.id
    : null;
}

type Translate = (key: string) => string;

const MATCH_LABELS: Record<string, string> = {
  matched: 'Library::Matched',
  ambiguous: 'Library::Needs review',
  unmatched: 'Library::No match',
  noGame: 'Library::Marked as not a game',
};

const STATUS_LABELS: Record<string, string> = {
  playing: 'Library::Playing',
  finished: 'Library::Finished',
  planned: 'Library::Planned',
  ongoing: 'Library::Ongoing',
  dropped: 'Library::Dropped',
  inLibrary: 'Library::In library',
  notInLibrary: 'Library::Not on GameNode',
};

const LAST_PLAYED_LABELS: Record<string, string> = {
  last7Days: 'Library::7 days',
  last30Days: 'Library::30 days',
  last90Days: 'Library::90 days',
  lastYear: 'Library::Last year',
  olderThanYear: 'Library::Older',
};

const ADDED_LABELS: Record<string, string> = {
  last7Days: 'Library::This week',
  last30Days: 'Library::This month',
  last90Days: 'Library::90 days',
  lastYear: 'Library::This year',
  olderThanYear: 'Library::Earlier',
};

const SORT_LABELS: Record<Exclude<SortKey, 'title'>, string> = {
  provider: 'Library::Store',
  matchStatus: 'Library::Match status',
  gameNodeStatus: 'Library::GameNode status',
  releaseYear: 'Library::Release year',
  playtime: 'Library::Playtime',
  lastPlayed: 'Library::Recently played',
  dateAdded: 'Library::Recently added',
};

const GROUP_LABELS: Record<GroupBy, string> = {
  none: 'Library::No grouping',
  store: 'Library::Store',
  gameNodeStatus: 'Library::GameNode status',
  matchState: 'Library::Match',
  installation: 'Library::Installation',
  releaseDecade: 'Library::Release decade',
};

function scopeLabel(
  scope: LibraryScope,
  views: SavedViewCount[],
  library: LibraryView | null | undefined,
  t: Translate,
): string {
  if (typeof scope === 'object') {
    if ('collection' in scope) {
      return (
        library?.collections.find((collection) => collection.id === scope.collection)?.name ??
        scope.collection
      );
    }
    return views.find((view) => view.id === scope.savedView)?.name ?? String(scope.savedView);
  }
  switch (scope) {
    case 'installed':
      return t('Sidebar::Installed');
    case 'favorites':
      return t('Sidebar::Favorites');
    case 'recent':
      return t('Sidebar::Recently played');
    case 'running':
      return t('Sidebar::Running');
    case 'all':
    default:
      return t('Sidebar::All games');
  }
}

function providerName(id: string, library: LibraryView | null | undefined): string {
  const provider = library?.providers.find((item) => item.id === id);
  return provider?.name ?? id;
}

function filterLabels(
  filters: LibraryFilters,
  library: LibraryView | null | undefined,
  t: Translate,
): string[] {
  const chips: string[] = [];

  const availability: string[] = [];
  if (filters.availability.installed) availability.push(t('Library::Installed'));
  if (filters.availability.owned) availability.push(t('Library::In library'));
  if (filters.availability.shared) availability.push(t('Library::Family shared'));
  if (availability.length > 0) {
    chips.push(`${t('Library::Availability')}: ${availability.join(', ')}`);
  }

  if (filters.stores.length > 0) {
    chips.push(
      `${t('Library::Store')}: ${filters.stores
        .map((id) => providerName(id, library))
        .join(', ')}`,
    );
  }

  if (filters.matchStates.length > 0) {
    chips.push(
      `${t('Library::Match')}: ${filters.matchStates
        .map((value) => t(MATCH_LABELS[value] ?? 'Library::Matched'))
        .join(', ')}`,
    );
  }

  if (filters.gamenodeStatus.length > 0) {
    chips.push(
      `${t('Library::GameNode status')}: ${filters.gamenodeStatus
        .map((value) => t(STATUS_LABELS[value] ?? 'Library::In library'))
        .join(', ')}`,
    );
  }

  if (filters.playtime !== 'any') {
    chips.push(
      `${t('Library::Playtime')}: ${
        filters.playtime === 'atLeast10h' ? t('Library::10 h+') : t('Library::100 h+')
      }`,
    );
  }

  if (filters.played !== 'any') {
    chips.push(
      `${t('Library::History')}: ${
        filters.played === 'played' ? t('Library::Played') : t('Library::Never played')
      }`,
    );
  }

  if (filters.lastPlayed !== 'any') {
    chips.push(
      `${t('Library::Last session')}: ${t(
        LAST_PLAYED_LABELS[filters.lastPlayed] ?? 'Library::Older',
      )}`,
    );
  }

  if (filters.release !== 'any') {
    const release =
      typeof filters.release === 'object'
        ? formatI18n(t('Library::{}s'), filters.release.decade)
        : filters.release === 'earlier'
          ? t('Library::Before 1990')
          : t('Library::Unknown');
    chips.push(`${t('Library::Release')}: ${release}`);
  }

  if (filters.added !== 'any') {
    chips.push(
      `${t('Library::Added')}: ${t(ADDED_LABELS[filters.added] ?? 'Library::Earlier')}`,
    );
  }

  if (filters.favoritesOnly) chips.push(t('Library::Favorites only'));
  if (filters.runningOnly) chips.push(t('Library::Running only'));
  if (filters.showHidden) chips.push(t('Library::Show hidden'));

  return chips;
}

function sortLabel(sort: Sort, t: Translate): string {
  if (sort.key === 'title') {
    return sort.direction === 'ascending'
      ? t('Library::Title (A–Z)')
      : t('Library::Title (Z–A)');
  }
  return t(SORT_LABELS[sort.key]);
}

function groupLabel(group: GroupBy, t: Translate): string {
  return t(GROUP_LABELS[group]);
}

interface ViewWriteAttempt {
  /** The `viewWrite.run` the attempt was submitted against. */
  baseline: number;
  operation: ViewWriteOperation;
  /** The renamed view id; 0 for a new save. */
  viewId: number;
  /** The trimmed name submitted. */
  name: string;
  /** The signed-in account the attempt belonged to; saved views are per account. */
  accountId: string | null;
}

/**
 * Save as view / rename dialog from `raw/06-L4-menus-popovers.txt` and task 19.3.
 *
 * The dialog validates the name live and only closes after the engine confirms the persisted
 * view through `viewWrite`: a success with the real database id, for the same account, run,
 * operation, view and name this attempt submitted. A refused name or a failed write stops the
 * spinner, keeps the dialog open and offers a retry. A sign-out or account switch before the
 * first `pending` snapshot ends the local attempt and clears the form, so neither a late result
 * nor a rejected IPC call can close or repaint another account's dialog. `lastSavedView` drives
 * the notification only, and the older global `viewError` cannot poison a new attempt.
 *
 * A rename is allowed even when the current layout is the default one: `canSaveView` only
 * gates creating a new view from a default payload.
 */
export function SaveViewModal({
  open: propOpen,
  library: propLibrary,
  renameViewId: propRenameViewId,
  accountId: propAccountId,
  onClose,
}: SaveViewModalProps) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const auth = useAuth();
  const saveViewOpen = useUiStore((s) => s.saveViewOpen);
  const storeRenameViewId = useUiStore((s) => s.renameViewId);

  const accountId =
    propAccountId !== undefined ? propAccountId : signedInAccountId(auth.auth);
  const isOpen = propOpen !== undefined ? propOpen : saveViewOpen;
  const renameViewId =
    propRenameViewId !== undefined ? propRenameViewId : storeRenameViewId;
  const renameTarget = renameViewId == null
    ? null
    : (library?.savedViews ?? []).find((view) => view.id === renameViewId) ?? null;

  const [name, setName] = useState('');
  const [touched, setTouched] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [writeError, setWriteError] = useState<ViewWriteError | null>(null);
  const fieldId = useId();
  const errorId = `${fieldId}-error`;
  const openKeyRef = useRef('');
  const attemptRef = useRef<ViewWriteAttempt | null>(null);
  const pendingSeenRef = useRef(false);
  const openRef = useRef(isOpen);
  const mountedRef = useRef(true);
  const writeRef = useRef(library?.viewWrite ?? null);
  const accountIdRef = useRef(accountId);
  const onCloseRef = useRef(onClose);

  openRef.current = isOpen;
  writeRef.current = library?.viewWrite ?? null;
  accountIdRef.current = accountId;
  onCloseRef.current = onClose;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Every opening starts from a clean local state. No result from a previous opening is
  // treated as this attempt's answer. The account is part of the opening identity: a sign-out or
  // an account switch always ends the local attempt, even before the engine's first `pending`
  // snapshot arrives, so the new account's form never inherits the old account's submission.
  useEffect(() => {
    if (!isOpen) {
      openKeyRef.current = '';
      return;
    }
    const key = `${accountId ?? 'anonymous'}:${renameViewId ?? 'new'}`;
    if (openKeyRef.current === key) return;
    openKeyRef.current = key;
    setName(renameTarget?.name ?? '');
    setTouched(false);
    setSubmitting(false);
    setWriteError(null);
    attemptRef.current = null;
    pendingSeenRef.current = false;
  }, [isOpen, accountId, renameViewId, renameTarget?.name]);

  const savedViews = library?.savedViews ?? [];
  const trimmed = name.trim();
  const duplicate = savedViews.some(
    (view) =>
      view.id !== renameViewId &&
      view.name.trim().toLowerCase() === trimmed.toLowerCase(),
  );
  const empty = trimmed.length === 0;
  const unchanged =
    renameTarget != null && trimmed === renameTarget.name;
  const showDuplicate = duplicate || writeError === 'duplicate';
  const showEmpty = empty && (touched || writeError === 'empty');
  const nameInvalid = showDuplicate || showEmpty || writeError === 'empty';
  const failureMessage =
    writeError === 'failed'
      ? t("Views::Couldn't save the view. Try again.")
      : null;
  const message =
    showDuplicate
      ? t('Views::A view with this name already exists')
      : showEmpty
        ? t('Views::Enter a name for this view.')
        : failureMessage;

  const summary = useMemo(() => {
    if (!library) return [] as string[];
    const chips = [scopeLabel(library.layout.scope, savedViews, library, t)];
    chips.push(...filterLabels(library.layout.filters, library, t));
    chips.push(formatI18n(t('Views::Sort: {}'), sortLabel(library.layout.sort, t)));
    chips.push(formatI18n(t('Views::Group: {}'), groupLabel(library.layout.group, t)));
    chips.push(
      library.layout.view === 'grid' ? t('Library::Grid') : t('Library::List'),
    );
    return chips;
  }, [library, savedViews, t]);

  const canSubmit =
    !submitting &&
    !empty &&
    !duplicate &&
    !unchanged &&
    (renameViewId == null
      ? library?.canSaveView !== false
      : renameTarget != null);

  const handleClose = () => {
    closeSaveView();
    onClose?.();
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setTouched(true);
    if (!canSubmit) return;

    const operation: ViewWriteOperation = renameViewId == null ? 'save' : 'rename';
    const attempt: ViewWriteAttempt = {
      baseline: writeRef.current?.run ?? 0,
      operation,
      viewId: renameViewId ?? 0,
      name: trimmed,
      accountId,
    };
    attemptRef.current = attempt;
    pendingSeenRef.current = false;
    setWriteError(null);
    setSubmitting(true);

    const input =
      renameViewId == null
        ? commands.libraryInput({ saveView: { name: trimmed } })
        : commands.libraryInput({ renameView: { id: renameViewId, name: trimmed } });

    // A rejected IPC never emits `viewWrite`, so this attempt would otherwise spin forever.
    // Only the attempt that is still open and current may fail locally; an engine answer that
    // already started a newer run is left to the result effect below.
    void input.catch(() => {
      if (!mountedRef.current || !openRef.current || attemptRef.current !== attempt) return;
      // A rejection caused by the old account's request must not paint an error on another
      // account's form, even before the engine reset has re-rendered this component.
      if (accountIdRef.current !== attempt.accountId) return;
      if ((writeRef.current?.run ?? 0) > attempt.baseline) return;
      attemptRef.current = null;
      setWriteError('failed');
      setSubmitting(false);
    });
  };

  // The engine answers every attempt with a new `viewWrite.run`, including repeated validation
  // refusals. Only this attempt's account, run, operation, target and trimmed name are accepted;
  // an older, stale or foreign-account result never closes or reopens the dialog. A success
  // closes only with the real database id, and a failure stops the spinner with a retry.
  useEffect(() => {
    if (!isOpen || !submitting) return;
    const attempt = attemptRef.current;
    if (!attempt) return;
    // `viewWrite` carries no account of its own: the account that submitted the attempt is what
    // binds the answer. A result that arrives after a sign-out or account switch cannot close the
    // new account's dialog, even when its operation, target and name coincide.
    if (attempt.accountId !== accountId) return;
    const write = library?.viewWrite ?? null;
    if (!write || write.run <= attempt.baseline) return;
    if (
      write.operation !== attempt.operation ||
      write.viewId !== attempt.viewId ||
      write.name !== attempt.name
    ) {
      return;
    }

    if (write.state === 'pending') {
      pendingSeenRef.current = true;
      return;
    }

    attemptRef.current = null;
    if (write.state === 'succeeded') {
      if (write.savedViewId == null) {
        // Never close on an unconfirmed success: the persisted id is the only proof. Report
        // it as a retryable failure instead of leaving the spinner waiting for a real id.
        setWriteError('failed');
        setSubmitting(false);
        return;
      }
      setSubmitting(false);
      setWriteError(null);
      closeSaveView();
      onCloseRef.current?.();
      return;
    }

    setWriteError(write.error ?? 'failed');
    setSubmitting(false);
  }, [isOpen, submitting, accountId, library?.viewWrite]);

  // A pending attempt that disappears was cleared by sign-out or an account change. The dialog
  // stays open, but it no longer claims to be saving. The account guard keeps an old account's
  // cleared write from reporting a failure on the new account's form.
  useEffect(() => {
    if (!isOpen || !submitting || !pendingSeenRef.current) return;
    const attempt = attemptRef.current;
    if (!attempt || attempt.accountId !== accountId) return;
    if (library?.viewWrite != null) return;
    attemptRef.current = null;
    pendingSeenRef.current = false;
    setWriteError('failed');
    setSubmitting(false);
  }, [isOpen, submitting, accountId, library?.viewWrite]);

  if (!isOpen) return null;

  return (
    <AppModal.Root
      opened
      onClose={handleClose}
      // Mantine converts numeric sizes to rem, which the 14 px app root would shrink to
      // 297.5 px. A calc() value passes through unchanged, so the dialog stays exactly 340 px.
      size="calc(340px)"
      transitionProps={{ duration: 0 }}
    >
      <AppModal.Overlay backgroundOpacity={0.62} />
      <AppModal.Content
        data-testid="save-view-modal"
        classNames={{
          // 340 px wide (the raw reference), never wider than the modal's own padded
          // viewport at small window sizes.
          content: 'w-[340px] max-w-full bg-secondary p-[16px]',
        }}
      >
        <form onSubmit={handleSubmit} noValidate className="flex flex-col gap-[12px]">
          <AppModal.Title className="text-[15px] font-semibold text-text-primary">
            {renameViewId == null ? t('Views::Save as view') : t('Views::Rename view')}
          </AppModal.Title>

          <div className="flex flex-col gap-[6px]">
            <label
              htmlFor={fieldId}
              className="text-xs font-medium text-text-secondary"
            >
              {t('Views::Name')}
            </label>
            <input
              id={fieldId}
              data-testid="save-view-name"
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                setTouched(true);
                setWriteError(null);
              }}
              required
              aria-invalid={nameInvalid || undefined}
              aria-describedby={message ? errorId : undefined}
              autoComplete="off"
              className="h-[34px] rounded-sm border border-border-subtle bg-surface px-[10px] text-[13px] text-text-primary outline-none focus:border-brand focus:ring-1 focus:ring-brand"
            />
          </div>

          <div className="flex flex-col gap-[6px] rounded-sm border border-border-subtle bg-body p-[10px]">
            <SectionLabel>{t('Views::Includes')}</SectionLabel>
            <div data-testid="save-view-summary" className="flex flex-wrap gap-[6px]">
              {summary.map((chip, index) => (
                <Pill key={`${chip}-${index}`} tone="neutral" className="max-w-full">
                  {chip}
                </Pill>
              ))}
            </div>
          </div>

          <div className="flex items-center gap-[8px] text-text-dimmed">
            <IconBookmark size={14} stroke={2} aria-hidden="true" />
            <span className="text-xs font-normal">
              {t('Views::Appears under “Collections and views” in the sidebar')}
            </span>
          </div>

          {message && (
            <p
              id={errorId}
              data-testid="save-view-validation"
              role="alert"
              className="text-xs text-danger"
            >
              {message}
            </p>
          )}

          <div className="flex justify-end gap-[8px]">
            <Button type="button" variant="ghost" size="sm" onClick={handleClose}>
              {t('Views::Cancel')}
            </Button>
            <Button type="submit" variant="primary" size="sm" loading={submitting} disabled={!canSubmit}>
              {renameViewId == null ? t('Views::Save view') : t('Views::Rename view')}
            </Button>
          </div>
        </form>
      </AppModal.Content>
    </AppModal.Root>
  );
}
