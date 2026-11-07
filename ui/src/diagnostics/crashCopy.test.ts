import { afterEach, describe, expect, it, vi } from 'vitest';
import { copyText } from './crashCopy';

function setClipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
}

function setExecCommand(impl: (command: string) => boolean) {
  Object.defineProperty(document, 'execCommand', { value: impl, configurable: true, writable: true });
}

describe('copyText', () => {
  afterEach(() => {
    delete (navigator as unknown as { clipboard?: unknown }).clipboard;
    delete (document as unknown as { execCommand?: unknown }).execCommand;
  });

  it('uses the async Clipboard API when it is available', async () => {
    const writeText = vi.fn((_text: string) => Promise.resolve());
    setClipboard(writeText);

    await expect(copyText('report body')).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith('report body');
  });

  it('falls back to execCommand when the Clipboard API refuses, and removes its textarea', async () => {
    setClipboard(() => Promise.reject(new Error('denied')));
    const execCommand = vi.fn((_command: string) => true);
    setExecCommand(execCommand);

    await expect(copyText('report body')).resolves.toBe(true);
    expect(execCommand).toHaveBeenCalledWith('copy');
    expect(document.querySelectorAll('textarea')).toHaveLength(0);
  });

  it('resolves to false when neither path works, without throwing', async () => {
    setClipboard(() => Promise.reject(new Error('denied')));
    setExecCommand(() => false);

    await expect(copyText('report body')).resolves.toBe(false);
  });

  it('resolves to false when execCommand throws', async () => {
    setExecCommand(() => {
      throw new Error('not allowed');
    });

    await expect(copyText('report body')).resolves.toBe(false);
  });
});
