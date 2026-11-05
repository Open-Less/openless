import { expect, type Locator, type Page } from '@playwright/test';

/** Geometry and control tests start after the explicit two-surface handoff;
 * transition tests sample that handoff separately. */
export async function settleNavigation(scope: Page | Locator) {
  await expect(scope.locator('.ol-transition-viewport[aria-busy="true"]')).toHaveCount(0);
}

/** Recording controls sit behind the microphone category, whose click unmounts the entry. */
export async function openMicrophoneSettings(scope: Page | Locator) {
  const heading = scope.getByRole('heading', { name: '麦克风与录音', exact: true });
  if (await heading.isVisible().catch(() => false)) return;
  const nav = scope.getByRole('navigation', { name: '设置分类' });
  if (await nav.isVisible().catch(() => false)) {
    await nav.getByRole('button', { name: '录音与输入', exact: true }).click();
  }
  if (await heading.isVisible().catch(() => false)) return;
  const entry = scope.locator('.ol-settings-category-entry').filter({ hasText: '麦克风与录音' });
  await expect(entry).toBeVisible();
  await entry.dispatchEvent('click');
  await expect(heading).toBeVisible();
}
