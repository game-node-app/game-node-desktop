import { describe, expect, it } from 'vitest';
import type { Snapshot } from '../../api';
import { createMockHost } from '../../mock/host';

/**
 * Proves the two halves of the catalog add contract against the real mock host:
 * the input the catalog view sends opens the existing Add dialog in catalog
 * mode, and confirming that run refreshes catalog membership without a second
 * search. The dialog itself stays the app-level `AddDialog`.
 */
describe('catalog add host contract', () => {
  it('opens catalog mode and flips inLibrary after the add completes', async () => {
    const host = createMockHost();

    await host.invoke('library_input', {
      input: { catalogSearch: 'Brass Meridian' },
    });
    // The mock search is asynchronous (250 ms), like the engine request.
    await new Promise((resolve) => setTimeout(resolve, 300));

    let snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const found = snapshot.library?.catalog.results.find(
      (game) => game.title === 'Brass Meridian',
    );
    expect(found).toBeTruthy();
    expect(found?.inLibrary).toBe(false);
    expect(found?.localKey).toBeNull();

    await host.invoke('library_input', {
      input: {
        openAddForGame: {
          gameId: found?.gameId,
          title: found?.title,
          coverUrl: found?.coverUrl,
        },
      },
    });

    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    const dialog = snapshot.library?.addDialog;
    expect(dialog?.mode).toBe('catalog');
    expect(dialog?.targets).toHaveLength(1);
    expect(dialog?.targets[0].target).toEqual({ game: found?.gameId });
    expect(dialog?.targets[0].entryKey).toBeNull();
    expect(dialog?.targets[0].matched).toBe(true);

    await host.invoke('library_input', { input: 'confirmAdd' });

    snapshot = (await host.invoke('get_snapshot')) as Snapshot;
    expect(snapshot.library?.addSummary?.added).toBe(1);
    const after = snapshot.library?.catalog.results.find(
      (game) => game.gameId === found?.gameId,
    );
    expect(after?.inLibrary).toBe(true);
    expect(after?.localKey).toBeNull();
  });
});
