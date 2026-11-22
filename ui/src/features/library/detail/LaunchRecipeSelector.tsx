import React from 'react';
import { commands, type LaunchRecipeChoices, type LaunchRecipeOption } from '../../../api';
import { useI18n } from '../../../i18n';

interface Props {
  entryKey: string;
}

const KNOWN_LAUNCH_LABELS: Record<string, string> = {
  'dev.richmont.gamenode.dolphin-emu-launch': 'Dolphin',
};

/** A human label for the extension that owns a recipe; recipe ids stay internal. */
function launchLabel(pluginId: string): string {
  const known = KNOWN_LAUNCH_LABELS[pluginId];
  if (known) return known;
  const tail = pluginId.split('.').pop() ?? pluginId;
  const words = tail.split(/[-_]/).filter(Boolean);
  return words.length > 0
    ? words.map((word) => word[0].toUpperCase() + word.slice(1)).join(' ')
    : pluginId;
}

export const LaunchRecipeSelector: React.FC<Props> = ({ entryKey }) => {
  const { t } = useI18n();
  const [choices, setChoices] = React.useState<LaunchRecipeChoices | null>(null);
  const [loading, setLoading] = React.useState(true);
  const [busy, setBusy] = React.useState(false);
  const [failed, setFailed] = React.useState(false);

  const reload = React.useCallback(async () => {
    setLoading(true);
    try {
      setChoices(await commands.launchRecipeChoices(entryKey));
      setFailed(false);
    } catch {
      setFailed(true);
    } finally {
      setLoading(false);
    }
  }, [entryKey]);

  React.useEffect(() => {
    void reload();
  }, [reload]);

  const choose = async (recipe: LaunchRecipeOption) => {
    if (!choices) return;
    setBusy(true);
    setFailed(false);
    try {
      await commands.selectLaunchRecipe(entryKey, recipe, choices.targetRevision);
      await reload();
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  const useDefault = async () => {
    if (!choices) return;
    setBusy(true);
    setFailed(false);
    try {
      await commands.useDetectedDefault(entryKey, choices.targetRevision);
      await reload();
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  const selected = choices?.selected ?? null;
  const selectedOption = selected
    ? choices?.recipes.find(
        (recipe) =>
          recipe.pluginId === selected.pluginId &&
          recipe.publisherFingerprint === selected.publisherFingerprint &&
          recipe.recipeId === selected.recipeId &&
          recipe.recipeRevision === selected.recipeRevision &&
          selected.baseTargetRevision === choices.targetRevision,
      )
    : undefined;
  // Historical proposals from failed attempts share the plugin; the user needs one method, not
  // one opaque recipe id per attempt. Keep the selected recipe when it still exists.
  const visibleRecipes = React.useMemo(() => {
    const byPlugin = new Map<string, LaunchRecipeOption>();
    for (const recipe of choices?.recipes ?? []) {
      if (!byPlugin.has(recipe.pluginId)) byPlugin.set(recipe.pluginId, recipe);
    }
    if (selectedOption) byPlugin.set(selectedOption.pluginId, selectedOption);
    return [...byPlugin.values()];
  }, [choices, selectedOption]);

  return (
    <section
      data-testid="launch-recipe-selector"
      aria-labelledby="launch-recipe-selector-title"
      className="flex flex-col gap-compact rounded-sm border border-border-subtle bg-raised p-sm"
    >
      <div id="launch-recipe-selector-title" className="text-xs font-semibold text-text-default">
        {t('Library::Launch method')}
      </div>
      {loading ? (
        <div role="status" className="text-xs text-text-dimmed">
          {t('Library::Loading launch methods…')}
        </div>
      ) : (
        <>
          {selected && (
            <div
              role={selectedOption ? 'status' : 'alert'}
              className={selectedOption ? 'text-xs text-text-dimmed' : 'text-xs text-danger'}
              data-testid="selected-launch-recipe-status"
            >
              {selectedOption
                ? t('Library::Custom launch method: {}').replace(
                    '{}',
                    launchLabel(selectedOption.pluginId),
                  )
                : t('Library::The selected recipe changed or is unavailable. Choose another recipe or return to the detected default.')}
            </div>
          )}
          <div className="flex flex-wrap gap-compact">
            {visibleRecipes.map((recipe) => {
              const active =
                recipe.pluginId === selected?.pluginId &&
                recipe.publisherFingerprint === selected.publisherFingerprint &&
                recipe.recipeId === selected.recipeId &&
                recipe.recipeRevision === selected.recipeRevision &&
                selected.baseTargetRevision === choices?.targetRevision;
              return (
                <button
                  key={`${recipe.pluginId}:${recipe.publisherFingerprint}:${recipe.recipeId}`}
                  type="button"
                  data-testid={`launch-recipe-${recipe.pluginId}-${recipe.recipeId}`}
                  aria-pressed={active}
                  disabled={busy || active}
                  onClick={() => void choose(recipe)}
                  className="self-start rounded-xs border border-border-subtle bg-transparent px-2.5 py-1 text-xs font-medium text-text-default transition-colors hover:bg-surface disabled:opacity-45"
                >
                  {launchLabel(recipe.pluginId)}
                </button>
              );
            })}
            {selected && (
              <button
                type="button"
                data-testid="use-detected-default"
                disabled={busy}
                onClick={() => void useDefault()}
                className="self-start rounded-xs border border-border-subtle bg-transparent px-2.5 py-1 text-xs font-medium text-text-default transition-colors hover:bg-surface disabled:opacity-45"
              >
                {t('Library::Use detected default')}
              </button>
            )}
          </div>
          {!selected && choices && visibleRecipes.length === 0 && (
            <div className="text-xs text-text-dimmed">
              {t('Library::No custom launch recipes are available for this game.')}
            </div>
          )}
          {selected && !choices?.detectedDefaultAvailable && (
            <div className="text-xs text-text-dimmed">
              {t('Library::The detected default is currently unavailable.')}
            </div>
          )}
        </>
      )}
      {failed && (
        <div role="alert" className="text-xs text-danger">
          {t('Library::Launch choices could not be saved or refreshed. Check for changes and try again.')}
        </div>
      )}
      {!loading && choices?.selected === null && choices && visibleRecipes.length === 0 && (
        <div className="sr-only" role="status">
          {choices.detectedDefaultAvailable
            ? t('Library::The detected default is selected.')
            : t('Library::No launch method is currently available.')}
        </div>
      )}
    </section>
  );
};
