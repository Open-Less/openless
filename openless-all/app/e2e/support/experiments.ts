import type { Locator, Page } from '@playwright/test';
import { settleNavigation } from './navigation';

export async function openExperiments(root: Page | Locator) {
  await root.getByRole('button', { name: '调试工具', exact: true }).click();
  await settleNavigation(root);
  await root.locator('[data-ol-advanced-entry="experiments"]').click();
  await settleNavigation(root);
}
