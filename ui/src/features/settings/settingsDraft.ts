import type { BackupSettings } from '../../api/types';
import type { SettingsDraft } from '../../store/uiStore';

export type BackupDraftField = 'automatic' | 'destination' | 'retention';

/** Localized row order used by the unsaved bar and the discard confirmation. */
export const BACKUP_DRAFT_FIELD_LABEL_KEYS: Record<BackupDraftField, string> = {
  automatic: 'settings.backups.protectAutomatically',
  destination: 'settings.backups.destination',
  retention: 'settings.backups.copiesKept',
};

/**
 * Fields whose draft value differs from the saved Backups settings, in row
 * order. A draft equal to the baseline has no pending changes.
 */
export function backupDraftChangedFields(
  draft: SettingsDraft | null,
  baseline: BackupSettings,
): BackupDraftField[] {
  if (!draft || draft.topic !== 'backups') return [];

  const fields: BackupDraftField[] = [];
  const { values } = draft;
  if (
    values.automatic !== undefined &&
    values.automatic !== baseline.automatic
  ) {
    fields.push('automatic');
  }
  if (
    values.destination !== undefined &&
    values.destination !== baseline.destination
  ) {
    fields.push('destination');
  }
  if (values.retention !== undefined && values.retention !== baseline.retention) {
    fields.push('retention');
  }
  return fields;
}

/** Effective value of one Backups field: pending draft, otherwise the saved one. */
export function backupDraftValue<K extends keyof BackupSettings>(
  draft: SettingsDraft | null,
  baseline: BackupSettings,
  field: K,
): BackupSettings[K] {
  if (draft && draft.topic === 'backups') {
    const value = draft.values[field];
    if (value !== undefined) return value as BackupSettings[K];
  }
  return baseline[field];
}

/** Rewrites one Backups draft value and clears the previous save error. */
export function updateBackupDraft(
  previous: SettingsDraft | null,
  patch: Partial<SettingsDraft['values']>,
): SettingsDraft {
  const values =
    previous && previous.topic === 'backups' ? previous.values : {};
  return {
    topic: 'backups',
    values: { ...values, ...patch },
  };
}
