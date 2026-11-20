import { IconExternalLink } from '@tabler/icons-react';
import type { Provider } from '../../api/types';
import { useI18n } from '../../i18n';

const PROVIDER_NAMES: Record<Provider, string> = {
  google: 'Google',
  discord: 'Discord',
  twitter: 'Twitter',
};

const PROVIDER_INITIALS: Record<Provider, string> = {
  google: 'G',
  discord: 'D',
  twitter: 'X',
};

export interface ProviderButtonProps {
  provider: Provider;
  onClick: () => void;
  disabled?: boolean;
}

/**
 * A full-width sign-in option: provider badge, label and the external-link hint.
 * The label is localized; the badge letter is decorative.
 */
export function ProviderButton({ provider, onClick, disabled }: ProviderButtonProps) {
  const { t } = useI18n();

  return (
    <button
      type="button"
      data-testid={`provider-${provider}`}
      disabled={disabled}
      onClick={onClick}
      className="flex h-[44px] w-full shrink-0 cursor-pointer items-center gap-sm rounded-md border border-border-subtle bg-raised px-3.5 text-left transition-colors hover:border-border-strong hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
    >
      <span
        aria-hidden="true"
        className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-segment bg-border-floating text-xs font-bold text-text-primary"
      >
        {PROVIDER_INITIALS[provider]}
      </span>
      <span className="min-w-0 flex-1 truncate text-sm font-medium text-text-primary">
        {t('auth.continueWith', { provider: PROVIDER_NAMES[provider] })}
      </span>
      <IconExternalLink
        size={14}
        stroke={2}
        aria-hidden="true"
        className="shrink-0 text-text-dimmed"
      />
    </button>
  );
}
