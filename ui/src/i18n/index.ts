import React, { createContext, useContext, useMemo, useState, useEffect } from 'react';
import { en, type TranslationCatalog, type TranslationKey } from './en';
import { ptBR } from './pt-BR';

export { en, ptBR, type TranslationCatalog, type TranslationKey };

export type SupportedLocale = 'en' | 'pt-BR';

export const catalogs: Record<SupportedLocale, TranslationCatalog> = {
  'en': en as unknown as TranslationCatalog,
  'pt-BR': ptBR,
};

export function normalizeLocale(raw?: string | null): SupportedLocale {
  if (!raw) return 'en';
  const normalized = raw.trim().toLowerCase().replace('_', '-');
  if (normalized === 'pt' || normalized.startsWith('pt-') || normalized.startsWith('pt_')) {
    return 'pt-BR';
  }
  return 'en';
}

export type InterpolationParams = Record<string, string | number> | Array<string | number>;

export function interpolate(template: string, params?: InterpolationParams): string {
  if (!params) return template;
  let result = template;
  if (Array.isArray(params)) {
    params.forEach((val, idx) => {
      result = result.replaceAll(`{${idx}}`, String(val));
    });
  } else {
    for (const [key, val] of Object.entries(params)) {
      result = result.replaceAll(`{${key}}`, String(val));
    }
  }
  return result;
}

export interface I18nContextValue {
  locale: SupportedLocale;
  setLocale: (locale: SupportedLocale | string) => void;
  t: (keyOrMsgid: string, params?: InterpolationParams) => string;
  catalog: TranslationCatalog;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export interface I18nProviderProps {
  initialLocale?: string;
  children?: React.ReactNode;
}

export const I18nProvider: React.FC<I18nProviderProps> = ({ initialLocale, children }) => {
  const [locale, setLocaleState] = useState<SupportedLocale>(() => normalizeLocale(initialLocale));

  const setLocale = (newLocale: SupportedLocale | string) => {
    setLocaleState(normalizeLocale(newLocale));
  };

  useEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = locale;
    }
  }, [locale]);

  const value = useMemo<I18nContextValue>(() => {
    const catalog = catalogs[locale] ?? catalogs.en;

    const t = (keyOrMsgid: string, params?: InterpolationParams): string => {
      const template =
        (catalog as Record<string, string>)[keyOrMsgid] ??
        (catalogs.en as Record<string, string>)[keyOrMsgid] ??
        keyOrMsgid;

      return interpolate(template, params);
    };

    return {
      locale,
      setLocale,
      t,
      catalog,
    };
  }, [locale]);

  return React.createElement(I18nContext.Provider, { value }, children);
};

export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) {
    throw new Error('useI18n must be used within an I18nProvider');
  }
  return ctx;
}
