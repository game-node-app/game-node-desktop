import { AppModal } from '../../components/AppModal';
import { KbdCombo, SectionLabel } from '../../components/ui';
import { closeShortcuts, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

interface ShortcutRow {
  labelKey: string;
  keys: string[];
}

interface ShortcutSection {
  id: string;
  titleKey: string;
  rows: ShortcutRow[];
}

/**
 * The shortcuts dialog (design D11 and `copy.md`). It is a regular `AppModal`, so it shares
 * the layer stack, focus trap, Escape and focus restore with the other shell dialogs. Key
 * names stay untranslated except the click key cap, which `copy.md` localizes.
 */
const SECTIONS: ShortcutSection[] = [
  {
    id: 'general',
    titleKey: 'Shortcuts::General',
    rows: [
      { labelKey: 'Shortcuts::Command palette', keys: ['Ctrl', 'K'] },
      { labelKey: 'Shortcuts::Settings', keys: ['Ctrl', ','] },
      { labelKey: 'Shortcuts::Rescan', keys: ['F5'] },
      { labelKey: 'Shortcuts::Quit', keys: ['Ctrl', 'Q'] },
    ],
  },
  {
    id: 'library',
    titleKey: 'Shortcuts::Library',
    rows: [
      { labelKey: 'Shortcuts::Search', keys: ['Ctrl', 'F'] },
      { labelKey: 'Shortcuts::Filters', keys: ['Ctrl', 'Shift', 'F'] },
      { labelKey: 'Shortcuts::Grid / list', keys: ['Ctrl', 'L'] },
    ],
  },
  {
    id: 'selected',
    titleKey: 'Shortcuts::Selected game',
    rows: [
      { labelKey: 'Shortcuts::Play', keys: ['Enter'] },
      { labelKey: 'Shortcuts::Favorite', keys: ['F'] },
      { labelKey: 'Shortcuts::Details panel', keys: ['Ctrl', 'I'] },
      { labelKey: 'Shortcuts::Context menu', keys: ['Shift', 'F10'] },
      { labelKey: 'Shortcuts::Select several', keys: ['Ctrl', 'Click'] },
      { labelKey: 'Shortcuts::Select all', keys: ['Ctrl', 'A'] },
    ],
  },
];

export interface ShortcutsModalProps {
  /** Injected in tests; the UI store decides otherwise. */
  open?: boolean;
  /** Injected in tests; the UI store decides otherwise. */
  onClose?: () => void;
}

export function ShortcutsModal({ open, onClose }: ShortcutsModalProps = {}) {
  const { t } = useI18n();
  const storeOpen = useUiStore((state) => state.shortcutsOpen);
  const opened = open ?? storeOpen;
  const handleClose = onClose ?? closeShortcuts;

  if (!opened) return null;

  return (
    <AppModal.Root opened onClose={handleClose} size={520} transitionProps={{ duration: 0 }}>
      <AppModal.Overlay backgroundOpacity={0.62} />
      <AppModal.Content
        data-testid="shortcuts-modal"
        classNames={{ content: 'w-[520px] max-w-[520px] bg-secondary p-5' }}
      >
        <div className="flex items-center justify-between gap-3">
          <AppModal.Title className="text-[15px] font-semibold text-text-primary">
            {t('Shortcuts::Keyboard shortcuts')}
          </AppModal.Title>
          <AppModal.CloseButton aria-label={t('Settings::Close')} />
        </div>

        <AppModal.Body className="mt-4 flex max-h-[min(560px,calc(100vh_-_180px))] flex-col gap-4 overflow-y-auto">
          {SECTIONS.map((section) => (
            <section
              key={section.id}
              data-testid={`shortcuts-section-${section.id}`}
              aria-labelledby={`shortcuts-${section.id}`}
              className="flex flex-col gap-1"
            >
              <SectionLabel id={`shortcuts-${section.id}`}>
                {t(section.titleKey)}
              </SectionLabel>
              {section.rows.map((row) => {
                const keys =
                  row.labelKey === 'Shortcuts::Select several'
                    ? ['Ctrl', t('Shortcuts::Click (key name)')]
                    : row.keys;
                return (
                  <div
                    key={row.labelKey}
                    className="flex h-8 items-center justify-between gap-3 rounded-control px-1.5 text-[13px] text-text-default"
                  >
                    <span className="min-w-0 truncate">{t(row.labelKey)}</span>
                    <KbdCombo keys={keys} />
                  </div>
                );
              })}
            </section>
          ))}
        </AppModal.Body>
      </AppModal.Content>
    </AppModal.Root>
  );
}
