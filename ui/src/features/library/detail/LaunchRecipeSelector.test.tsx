import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { commands, type LaunchRecipeChoices } from '../../../api';
import { I18nProvider } from '../../../i18n';
import { LaunchRecipeSelector } from './LaunchRecipeSelector';

const recipe = {
  pluginId: 'org.example.launcher',
  publisherFingerprint: 'ab'.repeat(32),
  recipeId: 'proton-9',
  recipeRevision: 3,
};

function selection(): NonNullable<LaunchRecipeChoices['selected']> {
  return {
    entryKey: 'steam:620',
    ownerKey: 'plugin-owner',
    ...recipe,
    baseTargetRevision: 7,
    approvedPlanFingerprint: '',
  };
}

describe('LaunchRecipeSelector', () => {
  afterEach(() => vi.restoreAllMocks());

  it.each([
    ['en', 'Launch method', 'Use detected default'],
    ['pt-BR', 'Método de inicialização', 'Usar padrão detectado'],
  ])('lets the user choose a recipe and return to the detected default in %s', async (locale, title, defaultLabel) => {
    let selected: LaunchRecipeChoices['selected'] = null;
    const choices = vi.spyOn(commands, 'launchRecipeChoices').mockImplementation(async () => ({
      targetRevision: 7,
      detectedDefaultAvailable: true,
      selected,
      recipes: [recipe],
    }));
    const choose = vi.spyOn(commands, 'selectLaunchRecipe').mockImplementation(async () => {
      selected = selection();
    });
    const useDefault = vi.spyOn(commands, 'useDetectedDefault').mockImplementation(async () => {
      selected = null;
    });
    const user = userEvent.setup();

    render(
      <I18nProvider initialLocale={locale}>
        <LaunchRecipeSelector entryKey="steam:620" />
      </I18nProvider>,
    );

    expect(await screen.findByText(title)).toBeInTheDocument();
    await user.click(screen.getByTestId('launch-recipe-org.example.launcher-proton-9'));
    await waitFor(() => expect(choose).toHaveBeenCalledWith('steam:620', recipe, 7));
    expect(await screen.findByTestId('selected-launch-recipe-status')).toHaveTextContent(
      'Launcher',
    );
    expect(
      screen.getByTestId('selected-launch-recipe-status'),
    ).not.toHaveTextContent('proton-9');

    await user.click(screen.getByRole('button', { name: defaultLabel }));
    await waitFor(() => expect(useDefault).toHaveBeenCalledWith('steam:620', 7));
    await waitFor(() => expect(choices).toHaveBeenCalledTimes(3));
    expect(screen.queryByTestId('use-detected-default')).not.toBeInTheDocument();
  });

  it('shows the runtime and launch arguments when the host provides a summary', async () => {
    const summary = 'Dolphin 2609 — -b -e Pikmin (USA) (Rev 1).rvz';
    let selected: LaunchRecipeChoices['selected'] = null;
    vi.spyOn(commands, 'launchRecipeChoices').mockImplementation(async () => ({
      targetRevision: 7,
      detectedDefaultAvailable: false,
      selected,
      recipes: [{ ...recipe, summary }],
    }));
    vi.spyOn(commands, 'selectLaunchRecipe').mockImplementation(async () => {
      selected = selection();
    });
    const user = userEvent.setup();

    render(
      <I18nProvider initialLocale="en">
        <LaunchRecipeSelector entryKey="steam:620" />
      </I18nProvider>,
    );

    const button = await screen.findByRole('button', { name: summary });
    expect(button).not.toHaveTextContent('proton-9');
    await user.click(button);
    expect(await screen.findByTestId('selected-launch-recipe-status')).toHaveTextContent(
      summary,
    );
  });

  it('shows one method per plugin and never a recipe id', async () => {
    vi.spyOn(commands, 'launchRecipeChoices').mockResolvedValue({
      targetRevision: 7,
      detectedDefaultAvailable: true,
      selected: null,
      recipes: [
        recipe,
        { ...recipe, recipeId: 'proton-10', recipeRevision: 4 },
      ],
    });
    render(
      <I18nProvider initialLocale="en">
        <LaunchRecipeSelector entryKey="steam:620" />
      </I18nProvider>,
    );

    const buttons = await screen.findAllByTestId(/^launch-recipe-org\.example\.launcher-/);
    expect(buttons).toHaveLength(1);
    expect(buttons[0]).toHaveTextContent('Launcher');
    expect(screen.queryByText(/proton-9/)).not.toBeInTheDocument();
    expect(screen.queryByText(/org\.example\.launcher/)).not.toBeInTheDocument();
  });

  it.each([
    ['en', 'Choose another recipe or return to the detected default', 'Use detected default'],
    ['pt-BR', 'Escolha outra receita ou volte ao padrão detectado', 'Usar padrão detectado'],
  ])('offers repair when the stored recipe is no longer available in %s', async (locale, alert, defaultLabel) => {
    vi.spyOn(commands, 'launchRecipeChoices').mockResolvedValue({
      targetRevision: 8,
      detectedDefaultAvailable: true,
      selected: { ...selection(), baseTargetRevision: 7 },
      recipes: [],
    });
    render(
      <I18nProvider initialLocale={locale}>
        <LaunchRecipeSelector entryKey="steam:620" />
      </I18nProvider>,
    );

    expect(await screen.findByRole('alert')).toHaveTextContent(alert);
    expect(screen.getByRole('button', { name: defaultLabel })).toBeEnabled();
  });
});
