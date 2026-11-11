import React, { useState } from 'react';
import { Switch } from '@mantine/core';
import { useI18n } from '../../i18n';
import { settingsCard, settingsRow, settingsRowDescription, settingsRowInfo, settingsRowTitle } from './settingsClasses';

export type CommonDataCategory = 'library' | 'playtime' | 'performance' | 'runtime';
export type SensitiveDataReview = 'source' | 'recipient' | 'fields' | 'history' | 'delivery' | 'retention';

export interface PluginActivationPermissionsValue {
  commonDataRead: CommonDataCategory[];
  sensitiveDataReview: SensitiveDataReview[];
}

interface Props {
  onContinue: (value: PluginActivationPermissionsValue) => void;
}

const COMMON_CATEGORIES: CommonDataCategory[] = ['library', 'playtime', 'performance', 'runtime'];
const SENSITIVE_REVIEW: SensitiveDataReview[] = ['source', 'recipient', 'fields', 'history', 'delivery', 'retention'];

export const PluginActivationPermissions: React.FC<Props> = ({ onContinue }) => {
  const { t } = useI18n();
  const [commonDataRead, setCommonDataRead] = useState<CommonDataCategory[]>([]);
  const [sensitiveDataReview, setSensitiveDataReview] = useState<SensitiveDataReview[]>([]);

  const toggle = <T extends string>(values: T[], value: T, update: (next: T[]) => void) => {
    update(values.includes(value) ? values.filter((item) => item !== value) : [...values, value]);
  };
  const label = (key: string) => t(`plugins.activation.${key}`);

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
            <Switch aria-label={label(`category.${category}`)} checked={commonDataRead.includes(category)} onChange={() => toggle(commonDataRead, category, setCommonDataRead)} color="brand" />
          </div>
        ))}
        <p className={`${settingsRowDescription} mt-compact border-t border-border-subtle pt-compact`}>
          {label('queryPolicy')}
        </p>
      </fieldset>
      <fieldset className={settingsCard}>
        <legend className={`${settingsRowTitle} px-1`}>{label('sensitiveTitle')}</legend>
        <p className={`${settingsRowDescription} mb-compact`}>{label('sensitiveDescription')}</p>
        {SENSITIVE_REVIEW.map((item) => (
          <div className={settingsRow} key={item}>
            <div className={settingsRowInfo}><span className={settingsRowTitle}>{label(`sensitive.${item}`)}</span></div>
            <Switch aria-label={label(`sensitive.${item}`)} checked={sensitiveDataReview.includes(item)} onChange={() => toggle(sensitiveDataReview, item, setSensitiveDataReview)} color="brand" />
          </div>
        ))}
      </fieldset>
      <button type="button" className="rounded-sm bg-brand px-md py-compact text-on-brand" onClick={() => onContinue({ commonDataRead, sensitiveDataReview })}>
        {label('continue')}
      </button>
    </section>
  );
};
