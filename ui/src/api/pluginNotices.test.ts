import { describe, expect, it } from 'vitest';
import {
  NOTICE_BODY_MAX,
  NOTICE_TITLE_MAX,
  noticeSource,
  pickText,
  plainText,
  type PluginNotice,
} from './pluginNotices';

function notice(texts: Array<[string, string, string]>): PluginNotice {
  return {
    pluginId: 'org.example.plugin',
    severity: 'info',
    texts: texts.map(([locale, title, body]) => ({ locale, title, body })),
  };
}

describe('pickText', () => {
  const full = notice([
    ['en', 'Backup finished', 'Saved.'],
    ['pt-BR', 'Backup concluído', 'Salvo.'],
    ['pt', 'Backup pronto', 'Salvo (pt).'],
    ['de', 'Sicherung fertig', 'Gespeichert.'],
  ]);

  it('prefers the exact locale', () => {
    expect(pickText(full, 'pt-BR').title).toBe('Backup concluído');
    expect(pickText(full, 'en').title).toBe('Backup finished');
    expect(pickText(full, 'de').title).toBe('Sicherung fertig');
  });

  it('ignores case and underscores when matching', () => {
    expect(pickText(full, 'PT-br').title).toBe('Backup concluído');
    expect(pickText(full, 'pt_BR').title).toBe('Backup concluído');
  });

  it('falls back to the same language, the region-less tag first', () => {
    expect(pickText(full, 'pt-PT').title).toBe('Backup pronto');
    const regional = notice([
      ['en', 'Main', ''],
      ['pt-BR', 'Brasil', ''],
    ]);
    // No plain "pt": any text in that language is better than another language.
    expect(pickText(regional, 'pt-PT').title).toBe('Brasil');
    expect(pickText(regional, 'pt').title).toBe('Brasil');
    const base = notice([
      ['en', 'Main', ''],
      ['pt', 'Português', ''],
    ]);
    expect(pickText(base, 'pt-BR').title).toBe('Português');
  });

  it('falls back to the plugin main text when nothing matches', () => {
    expect(pickText(full, 'fr').title).toBe('Backup finished');
    expect(pickText(full, '').title).toBe('Backup finished');
    expect(pickText(full, 'zh-Hant').title).toBe('Backup finished');
    const unlabelled = notice([['', 'Main', 'Body']]);
    expect(pickText(unlabelled, 'pt-BR')).toEqual({ locale: '', title: 'Main', body: 'Body' });
  });

  it('never picks a translation whose locale is empty or merely resembles the language', () => {
    const odd = notice([
      ['en', 'Main', ''],
      ['', 'No locale', ''],
      ['portuguese', 'Fake', ''],
    ]);
    expect(pickText(odd, 'pt-BR').title).toBe('Main');
  });

  it('survives malformed notices', () => {
    expect(pickText({ pluginId: 'x', severity: 'info', texts: [] }, 'en').title).toBe('');
    const broken = { pluginId: 'x', severity: 'info', texts: [null, { title: 5 }] } as unknown;
    expect(pickText(broken as PluginNotice, 'en').title).toBe('');
  });
});

describe('plainText', () => {
  it('removes controls, bidirectional overrides and invisible separators', () => {
    expect(plainText('a\u0000b\u001b[31mc\u007fd\u0085e', 99)).toBe('ab[31mcde');
    expect(plainText('exe \u202efdp.exe', 99)).toBe('exe fdp.exe');
    expect(plainText('a\u2066b\u2069c\u200bd\u2060e\ufeff', 99)).toBe('abcde');
    expect(plainText('t\u{e0041}ag', 99)).toBe('tag');
    // Ordinary joiners stay: emoji sequences and several scripts need them.
    expect(plainText('👨\u200d👩\u200d👧 می\u200cخواهم', 99)).toBe('👨\u200d👩\u200d👧 می\u200cخواهم');
  });

  it('keeps one line unless asked for several', () => {
    expect(plainText('one\ntwo\r\nthree', 99)).toBe('one two three');
    expect(plainText('one\ntwo\r\nthree\rfour', 99, true)).toBe('one\ntwo\nthree\nfour');
    expect(plainText('  padded  ', 99)).toBe('padded');
  });

  it('cuts to a length in characters, not bytes or code units', () => {
    expect(plainText('é'.repeat(10), 4)).toBe('éééé');
    expect(plainText('😀'.repeat(10), 3)).toBe('😀😀😀');
    expect(plainText('x'.repeat(1000), NOTICE_TITLE_MAX)).toHaveLength(NOTICE_TITLE_MAX);
    expect(plainText('x'.repeat(1000), NOTICE_BODY_MAX, true)).toHaveLength(NOTICE_BODY_MAX);
  });

  it('does not touch markup, links or script: they are only characters', () => {
    const hostile = '<script>alert(1)</script> [x](javascript:alert(2)) <a href="https://evil.test">y</a>';
    expect(plainText(hostile, 999, true)).toBe(hostile);
  });

  it('answers an empty string for anything that is not a string', () => {
    for (const value of [undefined, null, 5, {}, []]) {
      expect(plainText(value, 10)).toBe('');
    }
  });
});

describe('noticeSource', () => {
  it('labels with the plugin id, made plain', () => {
    expect(noticeSource(notice([['', 't', '']]))).toBe('org.example.plugin');
    const forged = { ...notice([['', 't', '']]), pluginId: 'Trusted \u202eevil' };
    expect(noticeSource(forged)).toBe('Trusted evil');
  });
});
