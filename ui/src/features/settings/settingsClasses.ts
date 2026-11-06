/**
 * Shared Tailwind utility strings for the Settings feature.
 *
 * These replace the former `settings.css` component classes. Colors that have no
 * dedicated Tailwind token (the settings secondary/muted text tones) reference the
 * Version 3.0 CSS variables from `src/styles/theme.css` so provenance is preserved.
 */

export const settingsSection = 'flex flex-col gap-sm';

export const settingsSectionTitle = 'text-md font-semibold text-white';

export const settingsSectionDescription =
  'text-sm text-[var(--settings-text-secondary)]';

export const settingsCard =
  'flex flex-col gap-sm rounded-sm border border-border-subtle bg-secondary p-md';

export const settingsCardRoomy =
  'flex flex-col gap-md rounded-sm border border-border-subtle bg-secondary p-md';

export const settingsRow = 'flex items-center justify-between gap-md';

export const settingsRowInfo = 'flex flex-col gap-[2px]';

export const settingsRowTitle = 'text-sm font-medium text-white';

export const settingsRowDescription =
  'text-xs text-[var(--settings-text-secondary)]';

const settingsButtonBase =
  'inline-flex items-center justify-center gap-[6px] rounded-sm px-sm py-[6px] text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50';

export const settingsButtonPrimary = `${settingsButtonBase} bg-brand text-white hover:bg-brand-4`;

export const settingsButtonSecondary = `${settingsButtonBase} border border-border-subtle bg-raised text-white hover:bg-[#2e2e2e]`;

export const settingsButtonDanger = `${settingsButtonBase} bg-danger text-white hover:bg-[#f05a5f]`;

export const settingsButtonGhost = `${settingsButtonBase} text-[var(--settings-text-secondary)] hover:bg-raised hover:text-white`;

export const settingsInput =
  'w-full rounded-sm border border-border-subtle bg-[var(--settings-bg-base)] px-[10px] py-[6px] text-sm text-white outline-none transition-colors focus:border-brand';

export const settingsPill =
  'inline-flex items-center rounded-full bg-raised px-compact py-[2px] text-xs font-medium text-[var(--settings-text-secondary)]';

export const settingsPillWarning =
  'inline-flex items-center rounded-full bg-warning-subtle px-compact py-[2px] text-xs font-medium text-warning';

const settingsInfoBoxBase =
  'flex items-center gap-compact rounded-sm border-l-[3px] bg-secondary px-[14px] py-[10px] text-xs';

export const settingsInfoBox = `${settingsInfoBoxBase} border-l-brand text-[var(--settings-text-secondary)]`;

export const settingsInfoBoxWarning = `${settingsInfoBoxBase} border-l-warning text-warning`;

export const settingsCloseButton =
  'flex items-center justify-center rounded-sm p-[6px] text-[var(--settings-text-secondary)] transition-colors hover:bg-raised hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand';
