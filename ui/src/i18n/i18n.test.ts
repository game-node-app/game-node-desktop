import React from 'react';
import { describe, it, expect } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import { en } from './en';
import { ptBR } from './pt-BR';
import { interpolate, normalizeLocale, catalogs, I18nProvider, useI18n } from './index';

describe('i18n catalogs', () => {
  it('en and pt-BR catalogs have 100% identical keys', () => {
    const enKeys = Object.keys(en).sort();
    const ptKeys = Object.keys(ptBR).sort();

    expect(enKeys.length).toBeGreaterThan(0);
    expect(ptKeys.length).toBe(enKeys.length);
    expect(ptKeys).toEqual(enKeys);
  });

  it('all translation values in en and pt-BR are non-empty strings', () => {
    for (const [key, val] of Object.entries(en)) {
      expect(typeof val).toBe('string');
      expect(val.trim().length, `empty en value for key: ${key}`).toBeGreaterThan(0);
    }

    for (const [key, val] of Object.entries(ptBR)) {
      expect(typeof val).toBe('string');
      expect(val.trim().length, `empty pt-BR value for key: ${key}`).toBeGreaterThan(0);
    }
  });

  it('keeps every PC ports key in both catalogs with matching placeholders', () => {
    const portsEnKeys = Object.keys(en)
      .filter((key) => key.startsWith('Ports::'))
      .sort();
    const portsPtKeys = Object.keys(ptBR)
      .filter((key) => key.startsWith('Ports::'))
      .sort();

    expect(portsEnKeys.length).toBeGreaterThan(0);
    expect(portsPtKeys).toEqual(portsEnKeys);

    const placeholders = (value: string) => (value.match(/\{[^}]*\}/g) ?? []).sort();
    // Words that are already the same in Brazilian Portuguese, plus the percent symbol.
    const identicalAllowed = new Set(['Ports::Console', 'Ports::Port', 'Ports::{}%']);

    for (const key of portsEnKeys) {
      const enValue = (en as Record<string, string>)[key];
      const ptValue = (ptBR as Record<string, string>)[key];
      expect(ptValue, `missing pt-BR value for ${key}`).toBeTruthy();
      expect(placeholders(ptValue), `placeholder mismatch for ${key}`).toEqual(
        placeholders(enValue),
      );
      if (!identicalAllowed.has(key)) {
        expect(ptValue, `untranslated pt-BR value for ${key}`).not.toBe(enValue);
      }
    }
  });

  it('supports named parameter interpolation for en and pt-BR', () => {
    const key = 'Skia failed to initialize: {gpu}\nGameNode could not restart with software rendering: {error}\nYour saved settings and session were preserved. Restart after updating your graphics driver.';

    const enTemplate = en[key as keyof typeof en];
    const ptTemplate = ptBR[key as keyof typeof ptBR];

    expect(enTemplate).toBeDefined();
    expect(ptTemplate).toBeDefined();

    const enInterpolated = interpolate(enTemplate, {
      gpu: 'Vulkan device lost',
      error: 'Out of memory',
    });
    expect(enInterpolated).toContain('Skia failed to initialize: Vulkan device lost');
    expect(enInterpolated).toContain('software rendering: Out of memory');

    const ptInterpolated = interpolate(ptTemplate, {
      gpu: 'Vulkan device lost',
      error: 'Out of memory',
    });
    expect(ptInterpolated).toContain('Falha ao iniciar o Skia: Vulkan device lost');
    expect(ptInterpolated).toContain('software: Out of memory');
  });

  it('supports indexed parameter interpolation ({0}, {1})', () => {
    const customTemplateEn = 'Game {0} updated to version {1}';
    const customTemplatePt = 'Jogo {0} atualizado para versão {1}';

    expect(interpolate(customTemplateEn, ['Cyberpunk', '2.1'])).toBe('Game Cyberpunk updated to version 2.1');
    expect(interpolate(customTemplatePt, ['Cyberpunk', '2.1'])).toBe('Jogo Cyberpunk atualizado para versão 2.1');
  });

  it('correctly normalizes locale strings', () => {
    expect(normalizeLocale('en')).toBe('en');
    expect(normalizeLocale('en-US')).toBe('en');
    expect(normalizeLocale('pt-BR')).toBe('pt-BR');
    expect(normalizeLocale('pt_BR')).toBe('pt-BR');
    expect(normalizeLocale('pt')).toBe('pt-BR');
    expect(normalizeLocale('PT-BR')).toBe('pt-BR');
    expect(normalizeLocale(null)).toBe('en');
    expect(normalizeLocale(undefined)).toBe('en');
  });

  it('catalogs dictionary contains both en and pt-BR', () => {
    expect(catalogs.en).toBeDefined();
    expect(catalogs['pt-BR']).toBeDefined();
  });

  it('renders translations in React components and supports runtime switching', () => {
    const TestConsumer = () => {
      const { t, locale, setLocale } = useI18n();
      return React.createElement(
        'div',
        null,
        React.createElement('span', { 'data-testid': 'greeting' }, t('Welcome to GameNode')),
        React.createElement('span', { 'data-testid': 'locale' }, locale),
        React.createElement(
          'button',
          {
            'data-testid': 'btn-pt',
            onClick: () => setLocale('pt_BR'),
          },
          'Switch PT'
        ),
        React.createElement(
          'button',
          {
            'data-testid': 'btn-en',
            onClick: () => setLocale('en'),
          },
          'Switch EN'
        )
      );
    };

    render(
      React.createElement(
        I18nProvider,
        { initialLocale: 'en' },
        React.createElement(TestConsumer, null)
      )
    );

    expect(screen.getByTestId('greeting').textContent).toBe('Welcome to GameNode');
    expect(screen.getByTestId('locale').textContent).toBe('en');

    act(() => {
      screen.getByTestId('btn-pt').click();
    });

    expect(screen.getByTestId('greeting').textContent).toBe('Boas-vindas ao GameNode');
    expect(screen.getByTestId('locale').textContent).toBe('pt-BR');

    act(() => {
      screen.getByTestId('btn-en').click();
    });

    expect(screen.getByTestId('greeting').textContent).toBe('Welcome to GameNode');
    expect(screen.getByTestId('locale').textContent).toBe('en');
  });
});
