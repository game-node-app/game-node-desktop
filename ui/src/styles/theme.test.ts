import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import postcss from 'postcss';
import tailwindcss from 'tailwindcss';
import { describe, expect, it } from 'vitest';
import tailwindConfig from '../../tailwind.config';

/**
 * The dialog redesign adds a small set of tokens. Each one must exist twice:
 * as a CSS variable in `theme.css` and as a Tailwind key that emits a class.
 * Compiling the utilities is the only check that proves a class name resolves;
 * asserting the variable declarations proves the class has a value at runtime.
 */
const NEW_CLASSES = [
  'rounded-dialog',
  'shadow-dialog',
  'text-dialog-title',
  'text-display',
  'text-hero',
  'leading-note',
  'leading-field',
  'leading-body-relaxed',
  'leading-hero',
  'bg-signin-side',
  'bg-signin-bottom',
] as const;

async function compileUtilities(classes: readonly string[]): Promise<string> {
  const config = {
    ...tailwindConfig,
    content: [{ raw: `<div class="${classes.join(' ')}"></div>` }],
  };
  const result = await postcss([tailwindcss(config)]).process(
    '@tailwind utilities;',
    { from: undefined },
  );
  return result.css;
}

describe('dialog redesign tokens', () => {
  it('emits every new utility class', async () => {
    const css = await compileUtilities(NEW_CLASSES);

    for (const className of NEW_CLASSES) {
      expect(css, className).toContain(`.${className}`);
    }
  });

  it('binds the new classes to the dialog variables and values', async () => {
    const css = await compileUtilities(NEW_CLASSES);

    expect(css).toContain('border-radius: var(--radius-dialog)');
    expect(css).toContain('--tw-shadow: var(--shadow-dialog)');
    expect(css).toContain('font-size: var(--font-size-dialog-title)');
    expect(css).toContain('font-size: var(--font-size-display)');
    expect(css).toContain('font-size: var(--font-size-hero)');
    expect(css).toContain('line-height: 17px');
    expect(css).toContain('line-height: 19px');
    expect(css).toContain('line-height: 20px');
    expect(css).toContain('line-height: 22px');
    expect(css).toContain('background-image: var(--gradient-signin-side)');
    expect(css).toContain('background-image: var(--gradient-signin-bottom)');
  });

  it('declares the variables with the design values', () => {
    const themeCss = readFileSync(
      resolve(process.cwd(), 'src/styles/theme.css'),
      'utf8',
    );

    expect(themeCss).toContain(
      '--shadow-dialog: 0 20px 48px rgba(0, 0, 0, 0.55);',
    );
    expect(themeCss).toContain('--radius-dialog: var(--radius-palette);');
    expect(themeCss).toContain('--font-size-dialog-title: 17px;');
    expect(themeCss).toContain('--font-size-display: 26px;');
    expect(themeCss).toContain('--font-size-hero: 28px;');
    expect(themeCss).toMatch(
      /--gradient-signin-side:\s*linear-gradient\(\s*90deg,\s*rgba\(17, 17, 17, 0\.25\) 0%,\s*rgba\(17, 17, 17, 0\.95\) 100%\s*\);/,
    );
    expect(themeCss).toMatch(
      /--gradient-signin-bottom:\s*linear-gradient\(\s*180deg,\s*rgba\(22, 22, 22, 0\.6\) 0%,\s*rgba\(22, 22, 22, 1\) 100%\s*\);/,
    );
  });
});
