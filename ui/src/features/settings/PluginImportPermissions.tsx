import React from 'react';
import type { PluginImportPermissionReview } from '../../api';
import { useI18n } from '../../i18n';
import {
  settingsPill,
  settingsPillWarning,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';
import {
  importPermissionAllowed,
  importPermissionLabel,
  importScopeLabel,
  type ImportPermissionDecisions,
} from './pluginImportReview';

interface PluginImportPermissionsProps {
  permissions: PluginImportPermissionReview[];
  decisions: ImportPermissionDecisions;
  onDecisionsChange: (decisions: ImportPermissionDecisions) => void;
  disabled?: boolean;
  ariaLabel: string;
  emptyLabel: string;
}

/**
 * Readable permission rows shared by the package and bundle import reviews. Required
 * permissions are locked with an explanation; optional ones default to allowed and show
 * what stops working before the user turns them off.
 */
export const PluginImportPermissions: React.FC<PluginImportPermissionsProps> = ({
  permissions,
  decisions,
  onDecisionsChange,
  disabled = false,
  ariaLabel,
  emptyLabel,
}) => {
  const { t } = useI18n();

  if (permissions.length === 0) {
    return <p className={settingsRowDescription}>{emptyLabel}</p>;
  }

  return (
    <ul className="mt-compact flex flex-col gap-compact" aria-label={ariaLabel}>
      {permissions.map((permission) => {
        const label = importPermissionLabel(permission.id, t);
        const allowed = importPermissionAllowed(permission, decisions);
        const impact = permission.impactIfDisabled || t('plugins.import.permissionImpactFallback');
        return (
          <li
            key={permission.id}
            className="rounded-sm border border-border-subtle bg-raised p-compact"
            data-permission-id={permission.id}
          >
            <div className="flex items-start justify-between gap-compact">
              <div className="min-w-0 flex-1">
                <p className={settingsRowTitle}>{label}</p>
                <p className="mt-1 flex flex-wrap items-center gap-1">
                  <span
                    className={`${settingsPill} max-w-full truncate`}
                    title={permission.scope}
                  >
                    {importScopeLabel(permission.scope, t)}
                  </span>
                  <span className={permission.required ? settingsPillWarning : settingsPill}>
                    {permission.required
                      ? t('plugins.import.requiredBadge')
                      : t('plugins.import.optionalBadge')}
                  </span>
                </p>
                <p className={`${settingsRowDescription} mt-1`}>{permission.reason}</p>
                {permission.required ? (
                  <p className={`${settingsRowDescription} mt-1`}>
                    {t('plugins.import.requiredLocked')}
                  </p>
                ) : !allowed ? (
                  <p className={`${settingsRowDescription} mt-1 text-warning`} role="note">
                    {impact}
                  </p>
                ) : null}
              </div>
              {!permission.required && (
                <input
                  type="checkbox"
                  role="switch"
                  className="mt-1 shrink-0"
                  aria-label={t('plugins.import.permissionToggle', { permission: label })}
                  checked={allowed}
                  disabled={disabled}
                  onChange={(event) => {
                    const next = { ...decisions };
                    if (event.currentTarget.checked) delete next[permission.id];
                    else next[permission.id] = false;
                    onDecisionsChange(next);
                  }}
                />
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
};
