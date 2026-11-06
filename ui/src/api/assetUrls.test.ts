import { afterEach, describe, expect, it, vi } from 'vitest';
import { artworkAssetUrl, coverAssetUrl } from './types';

const WEBVIEW2_USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36 Edg/130.0 WebView2/130.0';
const WEBKITGTK_USER_AGENT =
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15';

function useUserAgent(userAgent: string) {
  vi.stubGlobal('navigator', { ...globalThis.navigator, userAgent });
}

describe('gamenode-asset URLs', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('uses the custom scheme directly outside Windows', () => {
    useUserAgent(WEBKITGTK_USER_AGENT);
    expect(coverAssetUrl('119133.jpg')).toBe('gamenode-asset://localhost/cover/119133.jpg');
    expect(artworkAssetUrl(119133, 2)).toBe('gamenode-asset://localhost/artwork/119133?rev=2');
  });

  it('uses the http localhost mapping on Windows WebView2, which cannot load the bare scheme', () => {
    useUserAgent(WEBVIEW2_USER_AGENT);
    expect(coverAssetUrl('119133.jpg')).toBe('http://gamenode-asset.localhost/cover/119133.jpg');
    expect(artworkAssetUrl(119133, 2)).toBe('http://gamenode-asset.localhost/artwork/119133?rev=2');
  });

  it('encodes the cover file name and returns null without a cover', () => {
    useUserAgent(WEBVIEW2_USER_AGENT);
    expect(coverAssetUrl('a b.jpg')).toBe('http://gamenode-asset.localhost/cover/a%20b.jpg');
    expect(coverAssetUrl(null)).toBeNull();
    expect(artworkAssetUrl(null)).toBeNull();
  });
});
