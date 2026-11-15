import { AppModal } from '../../components/AppModal';
import { Button } from '../../components/ui';
import { commands } from '../../api/commands';
import { useLibraryView } from '../../api/useSnapshot';
import type { LibraryView } from '../../api/types';
import { useI18n } from '../../i18n';
import { formatI18n } from '../library/formatters';

export interface DeleteViewModalProps {
  open: boolean;
  viewId: number | null;
  /** Injected in tests; the snapshot view decides otherwise. */
  library?: LibraryView | null;
  onClose: () => void;
}

/**
 * Confirmation for deleting one saved view, from `Views::Delete view “{}”?`.
 * It shares the AppModal layer stack, focus trap, Escape handling and focus restore
 * with the save/rename dialog.
 */
export function DeleteViewModal({
  open,
  viewId,
  library: propLibrary,
  onClose,
}: DeleteViewModalProps) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const view = viewId == null
    ? null
    : (library?.savedViews ?? []).find((candidate) => candidate.id === viewId) ?? null;

  if (!open || viewId == null) return null;

  const handleConfirm = () => {
    commands.libraryInput({ deleteView: { id: viewId } });
    onClose();
  };

  return (
    <AppModal.Root opened onClose={onClose} size={400} transitionProps={{ duration: 0 }}>
      <AppModal.Overlay backgroundOpacity={0.62} />
      <AppModal.Content
        data-testid="delete-view-modal"
        classNames={{ content: 'w-[400px] max-w-[400px] bg-secondary p-4' }}
      >
        <div className="flex flex-col gap-4">
          <AppModal.Title className="text-[15px] font-semibold text-text-primary">
            {formatI18n(
              t('Views::Delete view “{}”?'),
              view?.name ?? String(viewId),
            )}
          </AppModal.Title>
          <div className="flex justify-end gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClose}>
              {t('Views::Cancel')}
            </Button>
            <Button
              type="button"
              variant="danger"
              size="sm"
              data-testid="delete-view-confirm"
              onClick={handleConfirm}
            >
              {t('Views::Delete')}
            </Button>
          </div>
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
}
