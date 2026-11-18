import React, { useEffect, useMemo, useState } from 'react';
import { Switch } from '@mantine/core';
import type {
  SharedDataCategory,
  SharedDataCatalogRecord,
  SharedDataSourcePolicy,
} from '../../api/types';
import { useI18n } from '../../i18n';
import { settingsCard, settingsRow, settingsRowDescription, settingsRowInfo, settingsRowTitle } from './settingsClasses';

export interface PluginActivationPermissionsValue {
  commonDataRead: SharedDataCategory[];
  selectedRecordIds: string[];
  sourcePolicies: SharedDataSourcePolicy[];
}

interface Props {
  pluginId: string;
  catalog?: SharedDataCatalogRecord[];
  initialValue?: PluginActivationPermissionsValue;
  busy?: boolean;
  onSave: (value: PluginActivationPermissionsValue) => void;
}

const COMMON_CATEGORIES: SharedDataCategory[] = [
  'gamePaths',
  'gamePerformanceHistory',
  'runtimeCatalog',
  'contentAvailability',
  'libraryRoots',
  'saveProfiles',
];

function policyKey(policy: Pick<SharedDataSourcePolicy, 'sourcePluginId' | 'sourcePublisherFingerprint' | 'datasetId' | 'class'>): string {
  return JSON.stringify([policy.sourcePluginId, policy.sourcePublisherFingerprint, policy.datasetId, policy.class]);
}

export const PluginActivationPermissions: React.FC<Props> = ({
  pluginId,
  catalog = [],
  initialValue,
  busy = false,
  onSave,
}) => {
  const { t } = useI18n();
  const [commonDataRead, setCommonDataRead] = useState<SharedDataCategory[]>(initialValue?.commonDataRead ?? []);
  const [selectedRecordIds, setSelectedRecordIds] = useState<string[]>(initialValue?.selectedRecordIds ?? []);
  const [sourcePolicies, setSourcePolicies] = useState<SharedDataSourcePolicy[]>(initialValue?.sourcePolicies ?? []);

  useEffect(() => {
    setCommonDataRead(initialValue?.commonDataRead ?? []);
    setSelectedRecordIds(initialValue?.selectedRecordIds ?? []);
    setSourcePolicies(initialValue?.sourcePolicies ?? []);
  }, [initialValue]);

  const datasets = useMemo(() => {
    const rows = new Map<string, {
      sourcePluginId: string;
      sourcePublisherFingerprint: string;
      datasetId: string;
      class: 'sensitive' | 'custom';
      fields: Set<string>;
      recordIds: Set<string>;
    }>();
    for (const record of catalog) {
      if (record.sourcePluginId === pluginId || (record.class !== 'sensitive' && record.class !== 'custom')) continue;
      const identity = {
        sourcePluginId: record.sourcePluginId,
        sourcePublisherFingerprint: record.sourcePublisherFingerprint,
        datasetId: record.datasetId,
        class: record.class,
      };
      const key = policyKey(identity);
      const row = rows.get(key) ?? { ...identity, fields: new Set<string>(), recordIds: new Set<string>() };
      record.fields.forEach((field) => row.fields.add(field));
      row.recordIds.add(record.recordId);
      rows.set(key, row);
    }
    for (const policy of sourcePolicies) {
      const key = policyKey(policy);
      const row = rows.get(key) ?? {
        sourcePluginId: policy.sourcePluginId,
        sourcePublisherFingerprint: policy.sourcePublisherFingerprint,
        datasetId: policy.datasetId,
        class: policy.class,
        fields: new Set<string>(),
        recordIds: new Set<string>(),
      };
      policy.fields.forEach((field) => row.fields.add(field));
      rows.set(key, row);
    }
    return [...rows.entries()].map(([key, row]) => ({ ...row, key, fields: [...row.fields].sort() }))
      .sort((left, right) => `${left.sourcePluginId}:${left.datasetId}`.localeCompare(`${right.sourcePluginId}:${right.datasetId}`));
  }, [catalog, pluginId, sourcePolicies]);

  const commonRecords = useMemo(() => {
    const records = catalog.filter((record) => record.class === 'common' && record.sourcePluginId !== pluginId);
    const seen = new Set(records.map((record) => record.recordId));
    for (const recordId of selectedRecordIds) {
      if (!seen.has(recordId)) records.push({
        sourcePluginId: t('plugins.activation.unknownSource'),
        sourcePublisherFingerprint: '',
        datasetId: '',
        class: 'common',
        category: null,
        recordId,
        fields: [],
      });
    }
    return records;
  }, [catalog, pluginId, selectedRecordIds, t]);

  const toggle = <T extends string>(values: T[], value: T, update: (next: T[]) => void) => {
    update(values.includes(value) ? values.filter((item) => item !== value) : [...values, value]);
  };
  const label = (key: string) => t(`plugins.activation.${key}`);
  const policyFor = (key: string) => sourcePolicies.find((policy) => policyKey(policy) === key);
  const updatePolicy = (key: string, patch: Partial<SharedDataSourcePolicy>) => {
    const dataset = datasets.find((row) => row.key === key);
    if (!dataset) return;
    setSourcePolicies((current) => {
      const existing = current.find((policy) => policyKey(policy) === key);
      const next = {
        sourcePluginId: dataset.sourcePluginId,
        sourcePublisherFingerprint: dataset.sourcePublisherFingerprint,
        datasetId: dataset.datasetId,
        class: dataset.class,
        fields: existing?.fields ?? [],
        history: existing?.history ?? false,
        live: existing?.live ?? false,
        ...existing,
        ...patch,
      } satisfies SharedDataSourcePolicy;
      const other = current.filter((policy) => policyKey(policy) !== key);
      if (next.fields.length === 0) return other;
      return [...other, next];
    });
  };

  return (
    <section className="space-y-md" aria-label={label('title')}>
      <div>
        <h3 className={settingsRowTitle}>{label('title')}</h3>
        <p className={settingsRowDescription}>{label('grantExplanation')}</p>
      </div>
      <fieldset className={settingsCard}>
        <legend className={`${settingsRowTitle} px-1`}>{label('commonTitle')}</legend>
        <p className={`${settingsRowDescription} mb-compact`}>{label('commonDescription')}</p>
        {COMMON_CATEGORIES.map((category) => (
          <div className={settingsRow} key={category}>
            <div className={settingsRowInfo}><span className={settingsRowTitle}>{label(`category.${category}`)}</span></div>
            <Switch aria-label={label(`category.${category}`)} checked={commonDataRead.includes(category)} onChange={() => toggle(commonDataRead, category, setCommonDataRead)} color="brand" disabled={busy} />
          </div>
        ))}
        <p className={`${settingsRowDescription} mt-compact border-t border-border-subtle pt-compact`}>
          {label('queryPolicy')}
        </p>
      </fieldset>

      <fieldset className={settingsCard}>
        <legend className={`${settingsRowTitle} px-1`}>{label('selectedRecordsTitle')}</legend>
        <p className={`${settingsRowDescription} mb-compact`}>{label('selectedRecordsDescription')}</p>
        {commonRecords.length === 0 ? <p className={settingsRowDescription}>{label('noSelectedRecordsAvailable')}</p> : commonRecords.map((record) => (
          <label className="flex items-start gap-compact py-1 text-sm" key={record.recordId}>
            <input
              type="checkbox"
              checked={selectedRecordIds.includes(record.recordId)}
              disabled={busy}
              onChange={() => toggle(selectedRecordIds, record.recordId, setSelectedRecordIds)}
            />
            <span>{record.datasetId || label('retainedRecord')}: {record.recordId} · {record.sourcePluginId}</span>
          </label>
        ))}
      </fieldset>

      <fieldset className={settingsCard}>
        <legend className={`${settingsRowTitle} px-1`}>{label('sensitiveTitle')}</legend>
        <p className={`${settingsRowDescription} mb-compact`}>{label('sensitiveDescription')}</p>
        {datasets.length === 0 ? <p className={settingsRowDescription}>{label('noDatasetsAvailable')}</p> : datasets.map((dataset) => {
          const current = policyFor(dataset.key);
          return (
            <section className="border-t border-border-subtle py-compact first:border-0" key={dataset.key} aria-label={`${dataset.sourcePluginId} ${dataset.datasetId}`}>
              <p className={settingsRowTitle}>{dataset.datasetId}</p>
              <p className={`${settingsRowDescription} break-all`}>{label('source')}: {dataset.sourcePluginId} · {dataset.sourcePublisherFingerprint} · {dataset.class}</p>
              {dataset.fields.map((field) => (
                <label className="flex items-center gap-compact py-1 text-sm" key={field}>
                  <input
                    type="checkbox"
                    checked={current?.fields.includes(field) ?? false}
                    disabled={busy}
                    onChange={(event) => {
                      const fields = new Set(current?.fields ?? []);
                      if (event.currentTarget.checked) fields.add(field);
                      else fields.delete(field);
                      updatePolicy(dataset.key, { fields: [...fields] });
                    }}
                  />
                  <span>{field}</span>
                </label>
              ))}
              <div className="flex flex-wrap gap-md pt-compact">
                <label className="flex items-center gap-compact text-sm">
                  <input type="checkbox" checked={current?.history ?? false} disabled={busy || !current?.fields.length} onChange={(event) => updatePolicy(dataset.key, { history: event.currentTarget.checked })} />
                  <span>{label('history')}</span>
                </label>
                <label className="flex items-center gap-compact text-sm">
                  <input type="checkbox" checked={current?.live ?? false} disabled={busy || !current?.fields.length} onChange={(event) => updatePolicy(dataset.key, { live: event.currentTarget.checked })} />
                  <span>{label('live')}</span>
                </label>
              </div>
            </section>
          );
        })}
        <p className={`${settingsRowDescription} mt-compact border-t border-border-subtle pt-compact`}>
          {label('retentionSeparate')}
        </p>
      </fieldset>

      <button type="button" className="rounded-sm bg-brand px-md py-compact text-on-brand" disabled={busy} onClick={() => onSave({ commonDataRead, selectedRecordIds, sourcePolicies })}>
        {label('save')}
      </button>
    </section>
  );
};
