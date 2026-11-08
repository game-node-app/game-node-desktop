import React from 'react';
import { commands } from '../../../api/commands';
import { useI18n } from '../../../i18n';

const QUIVER_URL = 'https://quiverlauncher.com';

/**
 * The catalog credit, text only. The upstream trademark policy allows truthful text
 * references but not its logo, so this is a link and never an image.
 */
export const PortCredit: React.FC<{ className?: string }> = ({ className }) => {
  const { t } = useI18n();
  return (
    <a
      data-testid="ports-credit"
      href={QUIVER_URL}
      target="_blank"
      rel="noreferrer"
      title={t('Ports::Open quiverlauncher.com in your browser')}
      onClick={(event) => {
        event.preventDefault();
        commands.openToolSource(QUIVER_URL);
      }}
      className={`text-xs text-text-dimmed underline decoration-border-subtle underline-offset-2 transition-colors hover:text-text-default ${className ?? ''}`}
    >
      {t('Ports::Port catalog by quiverlauncher.com')}
    </a>
  );
};
