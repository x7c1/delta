import { describe, expect, it } from 'vitest';
import {
  COMPOSER_MAX_HEIGHT,
  COMPOSER_MIN_HEIGHT,
  autoGrowGeometry,
} from './autoGrow';

describe('autoGrowGeometry', () => {
  it('holds the min height for content shorter than the min', () => {
    const { height, overflow } = autoGrowGeometry(COMPOSER_MIN_HEIGHT - 20);
    expect(height).toBe(COMPOSER_MIN_HEIGHT);
    expect(overflow).toBe(false);
  });

  it('grows with content between the min and the cap', () => {
    const mid = (COMPOSER_MIN_HEIGHT + COMPOSER_MAX_HEIGHT) / 2;
    const { height, overflow } = autoGrowGeometry(mid);
    expect(height).toBe(mid);
    expect(overflow).toBe(false);
  });

  it('grows monotonically as content grows, up to the cap', () => {
    const small = autoGrowGeometry(COMPOSER_MIN_HEIGHT + 10).height;
    const larger = autoGrowGeometry(COMPOSER_MIN_HEIGHT + 40).height;
    expect(larger).toBeGreaterThan(small);
  });

  it('caps the height and switches to internal scrolling past the cap', () => {
    const { height, overflow } = autoGrowGeometry(COMPOSER_MAX_HEIGHT + 200);
    expect(height).toBe(COMPOSER_MAX_HEIGHT);
    expect(overflow).toBe(true);
  });

  it('does not overflow exactly at the cap', () => {
    const { height, overflow } = autoGrowGeometry(COMPOSER_MAX_HEIGHT);
    expect(height).toBe(COMPOSER_MAX_HEIGHT);
    expect(overflow).toBe(false);
  });

  it('honors explicit min/max overrides', () => {
    expect(autoGrowGeometry(50, 20, 40)).toEqual({ height: 40, overflow: true });
    expect(autoGrowGeometry(10, 20, 40)).toEqual({ height: 20, overflow: false });
    expect(autoGrowGeometry(30, 20, 40)).toEqual({ height: 30, overflow: false });
  });
});
