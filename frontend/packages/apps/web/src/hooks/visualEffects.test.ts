import { describe, expect, it } from 'vitest';
import { isLinuxWebKit, resolveVisualEffects } from './visualEffects';

/**
 * Real user-agent strings for the environments the resolver must distinguish.
 * The `auto` setting resolves to `flat` ONLY for Linux WebKit (Epiphany /
 * WebKitGTK); every other environment stays `rich`.
 */
const UA = {
  // Epiphany on Linux (WebKitGTK): AppleWebKit token, no Chromium markers.
  epiphanyLinux:
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Epiphany/45.0 Safari/605.1.15',
  // Safari on macOS: AppleWebKit token, no Chromium markers, but not Linux.
  safariMac:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4.1 Safari/605.1.15',
  // Chrome on Linux: AppleWebKit AND Chrome/ marker → Chromium-family, rich.
  chromeLinux:
    'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36',
  // Firefox on Linux: Gecko, no AppleWebKit token → rich.
  firefoxLinux:
    'Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0',
} as const;

const PLATFORM = {
  linux: 'Linux x86_64',
  mac: 'MacIntel',
} as const;

describe('isLinuxWebKit', () => {
  it('is true only for a WebKit-engined UA on a Linux platform', () => {
    expect(isLinuxWebKit(UA.epiphanyLinux, PLATFORM.linux)).toBe(true);
  });

  it('is false for macOS Safari (WebKit but not Linux)', () => {
    expect(isLinuxWebKit(UA.safariMac, PLATFORM.mac)).toBe(false);
  });

  it('is false for Chrome on Linux (Chromium-family markers present)', () => {
    expect(isLinuxWebKit(UA.chromeLinux, PLATFORM.linux)).toBe(false);
  });

  it('is false for Firefox on Linux (no AppleWebKit token)', () => {
    expect(isLinuxWebKit(UA.firefoxLinux, PLATFORM.linux)).toBe(false);
  });
});

describe('resolveVisualEffects', () => {
  describe('auto resolves against the environment', () => {
    it('is flat on Epiphany / WebKitGTK Linux', () => {
      expect(resolveVisualEffects('auto', UA.epiphanyLinux, PLATFORM.linux)).toBe(
        'flat',
      );
    });

    it('is rich on macOS Safari', () => {
      expect(resolveVisualEffects('auto', UA.safariMac, PLATFORM.mac)).toBe('rich');
    });

    it('is rich on Chrome on Linux', () => {
      expect(resolveVisualEffects('auto', UA.chromeLinux, PLATFORM.linux)).toBe(
        'rich',
      );
    });

    it('is rich on Firefox on Linux', () => {
      expect(resolveVisualEffects('auto', UA.firefoxLinux, PLATFORM.linux)).toBe(
        'rich',
      );
    });
  });

  describe('explicit settings win over the platform on every UA', () => {
    const cases: [keyof typeof UA, keyof typeof PLATFORM][] = [
      ['epiphanyLinux', 'linux'],
      ['safariMac', 'mac'],
      ['chromeLinux', 'linux'],
      ['firefoxLinux', 'linux'],
    ];

    for (const [ua, platform] of cases) {
      it(`'on' is always rich (${ua})`, () => {
        expect(resolveVisualEffects('on', UA[ua], PLATFORM[platform])).toBe('rich');
      });
      it(`'off' is always flat (${ua})`, () => {
        expect(resolveVisualEffects('off', UA[ua], PLATFORM[platform])).toBe('flat');
      });
    }
  });
});
