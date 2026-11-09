import type { Locator, Page } from '@playwright/test';
import { settleNavigation } from './navigation';

export async function openExperiments(root: Page | Locator) {
  await root.getByRole('button', { name: '高级功能', exact: true }).click();
  await settleNavigation(root);
}
