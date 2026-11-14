import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getMockHost, mockEnabled } from './mock';

/** The Tauri event the host emits when a plugin asks Core to show a notification. */
export const PLUGIN_NOTICE_EVENT = 'plugin:notification';

export type PluginNoticeSeverity = 'info' | 'success' | 'warning' | 'error';

/** One version of a notification's text, in `locale` (empty when the plugin gave none). */
export interface PluginNoticeText {
  locale: string;
  title: string;
  body: string;
}

/**
 * A notification a plugin published. Everything in it is untrusted text: `pluginId` is the
 * authenticated plugin id (set by the host, not the plugin), and `texts` holds the plugin's main
 * text first, then its translations.
 */
export interface PluginNotice {
  pluginId: string;
  severity: PluginNoticeSeverity;
  texts: PluginNoticeText[];
}

/** The longest title and body shown, matching the host's bounds. */
export const NOTICE_TITLE_MAX = 120;
export const NOTICE_BODY_MAX = 512;

// Characters that can make text pretend to be something else: C0/C1 controls (line breaks are
// handled separately), bidirectional overrides, invisible separators and tag characters. The host
// refuses them; they are removed again here so a notification never depends on that check.
// eslint-disable-next-line no-control-regex
const CONTROLS = /[\u0000-\u0009\u000B-\u001F\u007F-\u009F]/g;
const DISGUISES =
  /[\u061C\u200B\u200E\u200F\u2028\u2029\u202A-\u202E\u2060\u2066-\u2069\uFEFF\uFFF9-\uFFFB]|[\u{E0000}-\u{E007F}]/gu;

/**
 * Makes untrusted text safe to show as plain text: no controls or disguising characters, at most
 * `max` characters, and (unless `multiline`) a single line. Markup is not touched: it is rendered
 * as text by React and never interpreted.
 */
export function plainText(raw: unknown, max: number, multiline = false): string {
  if (typeof raw !== 'string') return '';
  // Line endings are settled first, so a lone carriage return stays a break and is not deleted.
  let text = raw.replace(DISGUISES, '').replace(/\r\n?/g, '\n').replace(CONTROLS, '');
  if (!multiline) text = text.replace(/\n+/g, ' ');
  text = text.trim();
  return Array.from(text).slice(0, max).join('');
}

function language(locale: string): string {
  return locale.trim().toLowerCase().replace('_', '-').split('-')[0] ?? '';
}

/**
 * The text to show for the user's `locale`: an exact match, then the same language (the
 * region-less tag first), then the plugin's main text. Case does not matter. The result is already
 * made plain.
 */
export function pickText(notice: PluginNotice, locale: string): PluginNoticeText {
  const texts = notice.texts.filter((text) => typeof text?.title === 'string');
  const main = texts[0] ?? { locale: '', title: '', body: '' };
  const wanted = locale.trim().toLowerCase().replace('_', '-');
  const same = (text: PluginNoticeText) => text.locale.trim().toLowerCase().replace('_', '-');
  const chosen =
    texts.find((text) => text.locale !== '' && same(text) === wanted) ??
    texts.find((text) => text.locale !== '' && same(text) === language(locale)) ??
    texts.find((text) => text.locale !== '' && language(text.locale) === language(locale)) ??
    main;
  return {
    locale: chosen.locale,
    title: plainText(chosen.title, NOTICE_TITLE_MAX),
    body: plainText(chosen.body, NOTICE_BODY_MAX, true),
  };
}

/** The label a notification is attributed with: the plugin id, made plain. */
export function noticeSource(notice: PluginNotice): string {
  return plainText(notice.pluginId, 128);
}

/** Subscribes to plugin notifications; the mock host feeds the same handler. */
export async function subscribeToPluginNotices(
  handler: (notice: PluginNotice) => void,
): Promise<UnlistenFn> {
  if (mockEnabled) {
    return (await getMockHost()).subscribePluginNotices(handler);
  }
  return listen<PluginNotice>(PLUGIN_NOTICE_EVENT, (event) => handler(event.payload));
}
