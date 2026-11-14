import { test, expect } from '@playwright/test';

test('auxiliary window entries load without blank-page exceptions or startup video', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  for (const kind of [
    'qa',
    'selection-voice-intent',
    'voice-edit',
    'less-computer',
    'less-computer-glow',
    'capsule',
    'capsule-rail',
  ]) {
    await page.goto('/?window=' + kind);
    await page.waitForLoadState('networkidle');
    await expect(page.locator('.ol-first-frame')).toHaveCount(0);
    await expect(page.locator('video')).toHaveCount(0);
    // Active business surfaces render content; retired agent URLs remain inert.
    if (['qa', 'voice-edit'].includes(kind)) {
      expect(await page.locator('#root').textContent()).not.toBe('');
    }
    if (['less-computer', 'less-computer-glow'].includes(kind)) {
      await expect(page.locator('#root')).toBeEmpty();
    }
  }
  expect(errors).toEqual([]);
});
