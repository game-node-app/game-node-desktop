import { describe, expect, it } from 'vitest';
import type { BackupSettings } from '../../api/types';
import type { SettingsDraft } from '../../store/uiStore';
import {
  backupDraftChangedFields,
  backupDraftValue,
  updateBackupDraft,
} from './settingsDraft';

const baseline: BackupSettings = {
  automatic: true,
  destination: '/saved/backups',
  retention: 10,
};

describe('settingsDraft helpers', () => {
  it('has no changes without a draft or when every value matches the baseline', () => {
    expect(backupDraftChangedFields(null, baseline)).toEqual([]);
    expect(
      backupDraftChangedFields(
        { topic: 'backups', values: { ...baseline } },
        baseline,
      ),
    ).toEqual([]);
  });

  it('lists changed fields in row order', () => {
    const draft: SettingsDraft = {
      topic: 'backups',
      values: {
        automatic: false,
        destination: '/new',
        retention: 20,
      },
    };
    expect(backupDraftChangedFields(draft, baseline)).toEqual([
      'automatic',
      'destination',
      'retention',
    ]);
  });

  it('falls back to the saved value for untouched fields', () => {
    const draft: SettingsDraft = {
      topic: 'backups',
      values: { destination: '/new' },
    };
    expect(backupDraftValue(draft, baseline, 'automatic')).toBe(true);
    expect(backupDraftValue(draft, baseline, 'destination')).toBe('/new');
    expect(backupDraftValue(draft, baseline, 'retention')).toBe(10);
  });

  it('merges an edit into the existing draft', () => {
    const first = updateBackupDraft(null, { destination: '/new' });
    const second = updateBackupDraft(first, { automatic: false });
    expect(second).toEqual({
      topic: 'backups',
      values: { destination: '/new', automatic: false },
    });
  });
});
