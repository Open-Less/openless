import { expect, type Locator, type Page } from '@playwright/test';

/** Geometry and control tests start after the explicit two-surface handoff;
 * transition tests sample that handoff separately. */
export async function settleNavigation(scope: Page | Locator) {
  await expect(scope.locator('.ol-transition-viewport[aria-busy="true"]')).toHaveCount(0);
}
