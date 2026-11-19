import React from 'react';
import type {
  PluginCompatibilityDetails,
  PluginCompatibilityState,
  PluginTargetAssessment,
} from '../../api';
import { useI18n } from '../../i18n';
import { settingsPill, settingsPillSuccess, settingsPillWarning, settingsRowDescription, settingsRowTitle } from './settingsClasses';

type Translate = (key: string, params?: Record<string, string | number>) => string;

function verdictClass(state: PluginCompatibilityState): string {
  if (state === 'compatible') return settingsPillSuccess;
  if (state === 'incompatible') return settingsPillWarning;
  return settingsPill;
}

function libcLabel(libc: PluginCompatibilityDetails['libc'], t: Translate): string | null {
  if (!libc) return null;
  const family = t(`plugins.compat.libc.${libc.family}`);
  return 'version' in libc && libc.version ? `${family} ${libc.version}` : family;
}

/** Plain-language declarations from the signed target requirements; absent ones are omitted. */
function requirementLines(target: PluginTargetAssessment['target'], t: Translate): string[] {
  const lines: string[] = [];
  const windows = target.requires?.windows;
  if (windows?.product) lines.push(t('plugins.compat.requirement.windowsProduct', { value: t(`plugins.compat.product.${windows.product}`) }));
  if (windows?.min_version || windows?.max_version) {
    lines.push(t('plugins.compat.requirement.windowsVersion', { min: windows.min_version ?? '—', max: windows.max_version ?? '—' }));
  }
  if (windows?.min_build != null || windows?.max_build != null) {
    lines.push(t('plugins.compat.requirement.windowsBuild', { min: windows.min_build ?? '—', max: windows.max_build ?? '—' }));
  }
  const linux = target.requires?.linux;
  if (linux?.libc) {
    lines.push(t('plugins.compat.requirement.libc', {
      family: t(`plugins.compat.libc.${linux.libc.family === 'glibc' ? 'gnu' : 'musl'}`),
      version: linux.libc.min_version,
    }));
  }
  if (linux?.distributions?.length) {
    lines.push(t('plugins.compat.requirement.distributions', {
      value: linux.distributions.map((entry) => `${entry.id} ${entry.versions.join(', ')}`).join('; '),
    }));
  }
  return lines;
}

function testedLabels(target: PluginTargetAssessment['target']): string[] {
  return target.tested_on.map((entry) => [
    entry.distribution ?? entry.product,
    entry.version ?? entry.release,
  ].filter(Boolean).join(' ')).filter((label) => label.length > 0);
}

/** Whether the detected system matches an author-declared tested environment. Informational. */
function matchesTested(target: PluginTargetAssessment['target'], details: PluginCompatibilityDetails): boolean {
  return target.tested_on.some((entry) => {
    if (entry.distribution) {
      return entry.distribution === details.os && (!entry.version || entry.version === details.release);
    }
    if (entry.product) {
      return entry.product === details.product && (!entry.release || entry.release === details.release);
    }
    return false;
  });
}

function summaryKey(details: PluginCompatibilityDetails): string {
  if (details.ambiguous) return 'plugins.compat.summary.ambiguous';
  return `plugins.compat.summary.${details.state}`;
}

export interface PluginCompatibilityReviewProps {
  details: PluginCompatibilityDetails;
}

/**
 * Target requirements, the detected effective environment, the compatibility verdict with its
 * reasons, and the author-tested coverage. The tested list is shown separately and never changes
 * whether the package may be activated.
 */
export const PluginCompatibilityReview: React.FC<PluginCompatibilityReviewProps> = ({ details }) => {
  const { t } = useI18n();
  const unknown = t('plugins.compat.unknownValue');
  const environment: Array<[string, string]> = [
    [t('plugins.compat.os'), details.os ?? unknown],
    [t('plugins.compat.release'), details.release ?? unknown],
    ...(details.product && details.product !== 'unknown' ? [[t('plugins.compat.product'), t(`plugins.compat.product.${details.product}`)] as [string, string]] : []),
    ...(details.build != null ? [[t('plugins.compat.build'), String(details.build)] as [string, string]] : []),
    [t('plugins.compat.architecture'), details.architecture ?? unknown],
    [t('plugins.compat.abi'), details.abi ?? unknown],
    [t('plugins.compat.libc'), libcLabel(details.libc, t) ?? unknown],
    [t('plugins.compat.emulation'), t(`plugins.compat.emulation.${details.emulation}`)],
  ];
  const selected = details.targets.filter((entry) => entry.verdict.state === 'compatible');
  const compatibleUntested = details.state === 'compatible' && !details.ambiguous && selected.length === 1
    ? !matchesTested(selected[0].target, details)
    : false;

  return (
    <section className="mt-compact border-t border-border-subtle pt-compact" aria-label={t('plugins.compat.title')}>
      <div className="flex flex-wrap items-center gap-compact">
        <h4 className={settingsRowTitle}>{t('plugins.compat.title')}</h4>
        <span className={verdictClass(details.state)}>{t(`plugins.compat.verdict.${details.state}`)}</span>
      </div>
      <p className={settingsRowDescription} role="status">{t(summaryKey(details))}</p>
      {details.state === 'compatible' && !details.ambiguous && selected.length === 1 && (
        <p className={settingsRowDescription} role="status">
          {t(compatibleUntested ? 'plugins.compat.coverage.untested' : 'plugins.compat.coverage.tested')}
        </p>
      )}
      <p className={`${settingsRowTitle} mt-compact`}>{t('plugins.compat.environment')}</p>
      <dl className="grid grid-cols-[max-content_1fr] gap-x-compact gap-y-1 text-xs text-[var(--settings-text-secondary)]">
        {environment.map(([label, value]) => (
          <React.Fragment key={label}>
            <dt>{label}</dt>
            <dd className="break-all">{value}</dd>
          </React.Fragment>
        ))}
      </dl>
      <p className={`${settingsRowTitle} mt-compact`}>{t('plugins.compat.targets')}</p>
      <ul className="flex flex-col gap-compact">
        {details.targets.map((entry) => {
          const requirements = requirementLines(entry.target, t);
          const tested = testedLabels(entry.target);
          return (
            <li key={entry.target.entrypoint} className="rounded-sm border border-border-subtle p-sm" aria-label={`${entry.target.os} ${entry.target.arch} ${entry.target.abi}`}>
              <div className="flex flex-wrap items-center gap-compact">
                <span className={settingsRowTitle}>{entry.target.os} · {entry.target.arch} · {entry.target.abi}</span>
                <span className={verdictClass(entry.verdict.state)}>{t(`plugins.compat.verdict.${entry.verdict.state}`)}</span>
              </div>
              <p className={`${settingsRowDescription} mt-1`}>{t('plugins.compat.requirements')}: {requirements.length ? requirements.join(' · ') : t('plugins.compat.requirementsNone')}</p>
              <ul className="mt-1 list-disc pl-md text-xs text-[var(--settings-text-secondary)]">
                {entry.verdict.reasons.map((reason) => <li key={reason}>{t(`plugins.compat.reason.${reason}`)}</li>)}
              </ul>
              {entry.verdict.unknownFacts.length > 0 && (
                <p className={`${settingsRowDescription} mt-1`}>{t('plugins.compat.unknownFacts')}: {entry.verdict.unknownFacts.join(', ')}</p>
              )}
              <p className={`${settingsRowDescription} mt-1`}>
                {t('plugins.compat.tested')}: {tested.length ? tested.join('; ') : t('plugins.compat.testedNone')}
              </p>
            </li>
          );
        })}
      </ul>
    </section>
  );
};
