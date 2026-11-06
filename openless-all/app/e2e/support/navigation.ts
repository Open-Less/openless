import { expect, type Locator, type Page } from '@playwright/test';

/** Geometry and control tests start after the explicit two-surface handoff;
 * transition tests sample that handoff separately. */
export async function settleNavigation(scope: Page | Locator) {
  await expect(scope.locator('.ol-transition-viewport[aria-busy="true"]')).toHaveCount(0);
}

/** Microphone controls live on a subpage. Opening that category unmounts its
 * entry, and the subpage heading stays at opacity 0 during the content fade.
 * A visibility probe therefore cannot tell the two surfaces apart: branch on
 * which node is mounted, and click the entry only while it is still mounted. */
export async function openMicrophoneSettings(scope: Page | Locator) {
  const heading = scope.getByRole('heading', { name: '麦克风与录音', exact: true });
  // Role queries skip aria-hidden and opacity 0. These CSS queries still see
  // a surface that is mounted but not yet painted.
  const subpage = scope.locator('h2').filter({ hasText: '麦克风与录音' });
  const entry = scope.locator('.ol-settings-category-entry').filter({ hasText: '麦克风与录音' });

  if ((await subpage.count()) === 0 && (await entry.count()) === 0) {
    const nav = scope.getByRole('navigation', { name: '设置分类' });
    await nav.getByRole('button', { name: '通用', exact: true }).click();
  }
  // An in-flight section handoff keeps the incoming page inert until it settles.
  await settleNavigation(scope);
  if ((await entry.count()) > 0) {
    await expect(entry).toBeVisible();
    // locator.click() retries the entry after this click unmounts it.
    await entry.dispatchEvent('click');
  }
  await expect(heading).toBeVisible();
}
