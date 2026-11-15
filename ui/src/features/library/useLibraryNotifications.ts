import { useEffect, useRef } from 'react';
import { commands } from '../../api/commands';
import type { AuthView, LaunchStatus } from '../../api/types';
import { useAuth, useLibraryView, useLocalGames, useSnapshot } from '../../api/useSnapshot';
import { useI18n } from '../../i18n';
import {
  dismissToast,
  pushNotification,
  type ToastAction,
  type ToastKind,
} from '../../store/uiStore';
import { formatI18n, formatStatus } from './formatters';

/** One account's already-announced runs. Run ids, never object identity, drive the dedupe. */
interface SeenRuns {
  accountId: string | null;
  initialized: boolean;
  librarySeen: boolean;
  add: number | null;
  undo: number | null;
  status: number | null;
  collection: number | null;
  savedView: number | null;
  hidden: number | null;
  launch: string | null;
}

function emptySeen(accountId: string | null): SeenRuns {
  return {
    accountId,
    initialized: false,
    librarySeen: false,
    add: null,
    undo: null,
    status: null,
    collection: null,
    savedView: null,
    hidden: null,
    launch: null,
  };
}

function signedInAccountId(auth: AuthView): string | null {
  return typeof auth === 'object' && auth !== null && 'signedIn' in auth
    ? auth.signedIn.user.id
    : null;
}

/** A launch failure's identity. Two identical failures in a row are one notification. */
function launchFailureSignature(launch: LaunchStatus | null | undefined): string | null {
  if (!launch || typeof launch.state !== 'object' || !('failed' in launch.state)) {
    return null;
  }
  return `${launch.entryKey}\u0000${launch.kind}\u0000${String(launch.state.failed)}`;
}

/** The core reports a missing DRM-free executable as `incomplete`. */
function isMissingExecutable(launch: LaunchStatus | null | undefined): boolean {
  if (!launch || typeof launch.state !== 'object' || !('failed' in launch.state)) {
    return false;
  }
  return launch.state.failed === 'incomplete';
}

function manualRegistrationId(entryKey: string): string | null {
  return entryKey.startsWith('manual:') ? entryKey.slice('manual:'.length) : null;
}

/** Commands are fire-and-forget here; a rejected IPC call must never become an unhandled rejection. */
function runCommand(command: () => Promise<unknown>): void {
  void command().catch(() => undefined);
}

function notificationId(scope: string, accountId: string | null, run: number | string): string {
  return `library-${scope}-${accountId ?? 'anonymous'}-${run}`;
}

function skippedNotOnGameNode(run: {
  skippedUnmatched: number;
  skippedNotInLibrary: number;
}): number {
  return run.skippedUnmatched + run.skippedNotInLibrary;
}

function joinBody(parts: Array<string | null>): string | undefined {
  const kept = parts.filter((part): part is string => Boolean(part));
  return kept.length > 0 ? kept.join(' · ') : undefined;
}

const selectLaunch = (snapshot: { launch?: LaunchStatus | null }) => snapshot.launch;

/**
 * Turns finished engine runs into D10 notifications, once per run and account.
 *
 * Mount this exactly once (the shell does it in `App`). It intentionally keeps its own
 * in-memory refs: opening or closing a dialog, switching tabs or remounting the hook must not
 * replay an old run, and the latest summary survives its dialog closing.
 */
export function useLibraryNotifications(): void {
  const library = useLibraryView();
  const auth = useAuth();
  const launch = useSnapshot(selectLaunch).data;
  const localGames = useLocalGames();
  const { t } = useI18n();

  const accountId = signedInAccountId(auth.auth);
  const seen = useRef<SeenRuns>(emptySeen(accountId));
  /** The add run whose success notification was shown; undo only reports for that run. */
  const notifiedAddRun = useRef<number | null>(null);

  // Baseline on mount, on the first library view and whenever the account changes. Marking the
  // current runs as seen is what prevents an old summary from being replayed.
  useEffect(() => {
    const state = seen.current;
    const accountChanged = state.accountId !== accountId;
    const needsBaseline =
      !state.initialized || accountChanged || (!state.librarySeen && library != null);
    if (!needsBaseline) return;

    seen.current = {
      accountId,
      initialized: true,
      librarySeen: Boolean(library),
      add: library?.addSummary?.runId ?? null,
      undo: library?.undo?.run ?? null,
      status: library?.statusRun?.run ?? null,
      collection: library?.collectionRun?.run ?? null,
      savedView: library?.lastSavedView?.run ?? null,
      hidden: library?.lastHidden?.run ?? null,
      launch: launchFailureSignature(launch),
    };
    if (accountChanged) {
      notifiedAddRun.current = null;
    }
  }, [accountId, library, launch]);

  useEffect(() => {
    const state = seen.current;
    if (!state.initialized || state.accountId !== accountId || !library) return;

    const add = library.addSummary;
    if (add && state.add !== add.runId) {
      state.add = add.runId;
      // Only a run that actually created entries is a legitimate success, and only it can be
      // undone. A pure edit or an already-present entry stays silent.
      if (add.added > 0) {
        notifiedAddRun.current = add.runId;
        const id = notificationId('add', accountId, add.runId);
        const actions: ToastAction[] = [];
        if (add.undoable) {
          actions.push({
            label: t('Notify::Undo'),
            onClick: () => {
              dismissToast(id);
              runCommand(() => commands.libraryInput({ undoAdd: { run: add.runId } }));
            },
          });
        }
        actions.push({
          label: t('Notify::View on GameNode'),
          onClick: () => {
            runCommand(() => commands.openGameNodePage('library'));
          },
        });
        pushNotification({
          id,
          kind: 'success',
          title:
            add.added === 1
              ? t('Notify::1 game added to GameNode')
              : formatI18n(t('Notify::{} games added to GameNode'), add.added),
          // The add dialog's existing summary carries the already-present count; the Notify
          // copy has no plural key for it, so use that established sentence when it applies.
          body:
            add.alreadyPresent > 0
              ? formatI18n(
                  t('Library::Saved {} · already in your library {} · skipped {} · failed {}'),
                  add.added,
                  add.alreadyPresent,
                  add.skipped,
                  add.failed,
                )
              : joinBody([
                  add.skipped > 0
                    ? formatI18n(t('Notify::{} not on GameNode were skipped'), add.skipped)
                    : null,
                  add.failed > 0 ? t("Notify::Couldn't update GameNode") : null,
                ]),
          actions,
        });
      }
    }

    const undo = library.undo;
    if (undo && state.undo !== undo.run) {
      state.undo = undo.run;
      // A stale undo (for an add this hook never announced) is ignored.
      if (notifiedAddRun.current === undo.run) {
        pushNotification({
          id: notificationId('undo', accountId, undo.run),
          kind: undo.state === 'done' ? 'success' : 'danger',
          title:
            undo.state === 'done'
              ? t('Notify::Add undone')
              : t("Notify::Couldn't undo the add"),
        });
      }
    }

    const statusRun = library.statusRun;
    if (statusRun && state.status !== statusRun.run) {
      state.status = statusRun.run;
      const kind: ToastKind = statusRun.failed > 0 ? 'danger' : statusRun.changed > 0 ? 'success' : 'info';
      pushNotification({
        id: notificationId('status', accountId, statusRun.run),
        kind,
        title: formatI18n(
          t('Notify::Status changed to {} for {} games'),
          formatStatus(statusRun.status, t),
          statusRun.changed,
        ),
        body: joinBody([
          skippedNotOnGameNode(statusRun) > 0
            ? formatI18n(
                t('Notify::{} not on GameNode were skipped'),
                skippedNotOnGameNode(statusRun),
              )
            : null,
          statusRun.failed > 0 ? t("Notify::Couldn't update GameNode") : null,
        ]),
      });
    }

    const collectionRun = library.collectionRun;
    if (collectionRun && state.collection !== collectionRun.run) {
      state.collection = collectionRun.run;
      const kind: ToastKind =
        collectionRun.failed > 0
          ? 'danger'
          : collectionRun.changed > 0
            ? 'success'
            : 'info';
      pushNotification({
        id: notificationId('collection', accountId, collectionRun.run),
        kind,
        title: formatI18n(t('Notify::Added to {}'), collectionRun.collectionName),
        body: joinBody([
          skippedNotOnGameNode(collectionRun) > 0
            ? formatI18n(
                t('Notify::{} not on GameNode were skipped'),
                skippedNotOnGameNode(collectionRun),
              )
            : null,
          collectionRun.failed > 0 ? t("Notify::Couldn't update GameNode") : null,
        ]),
      });
    }

    const savedView = library.lastSavedView;
    if (savedView && state.savedView !== savedView.run) {
      state.savedView = savedView.run;
      pushNotification({
        id: notificationId('view', accountId, savedView.run),
        kind: 'success',
        title: formatI18n(t('Notify::View “{}” saved'), savedView.name),
      });
    }

    const hidden = library.lastHidden;
    if (hidden && state.hidden !== hidden.run) {
      state.hidden = hidden.run;
      if (hidden.hidden && hidden.keys.length > 0) {
        const id = notificationId('hidden', accountId, hidden.run);
        pushNotification({
          id,
          kind: 'info',
          title: formatI18n(t('Notify::{} hidden from the library'), hidden.keys.length),
          actions: [
            {
              label: t('Notify::Show again'),
              onClick: () => {
                dismissToast(id);
                runCommand(() =>
                  commands.libraryInput({ setHidden: { keys: hidden.keys, hidden: false } }),
                );
              },
            },
          ],
        });
      }
    }

    if (!launch) {
      state.launch = null;
      return;
    }

    const signature = launchFailureSignature(launch);
    if (!signature) {
      // A later retry or a successful launch makes the next identical failure new again.
      state.launch = null;
      return;
    }
    if (state.launch !== signature) {
      state.launch = signature;
      if (isMissingExecutable(launch)) {
        const registrationId = manualRegistrationId(launch.entryKey);
        const title =
          library.paletteIndex.find((entry) => entry.key === launch.entryKey)?.title ??
          library.rows.find((row) => row.key === launch.entryKey)?.title ??
          localGames.registrations.find((registration) => registration.id === registrationId)
            ?.title ??
          launch.entryKey;

        pushNotification({
          id: notificationId('launch', accountId, signature),
          kind: 'danger',
          title: formatI18n(t("Notify::Couldn't start {}"), title),
          body: t('Notify::The executable was not found.'),
          actions: [
            {
              label: t('Notify::Locate executable'),
              onClick: () => {
                if (registrationId) {
                  runCommand(() => commands.pickLocalExecutable(registrationId));
                }
              },
            },
            {
              label: t('Notify::View log'),
              onClick: () => {
                runCommand(() => commands.openLogsFolder());
              },
            },
          ],
        });
      }
    }
  }, [accountId, launch, library, localGames.registrations, t]);
}
