import { expect, test } from '@playwright/test';
import { openExperiments } from './support/experiments';

test('retired agent features are absent and experiments only mount their active subpage', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await openExperiments(dialog);
  await expect(
    dialog.locator(
      '[data-ol-advanced-entry="testingFeatures"], [data-ol-advanced-entry="lessComputer"], [data-ol-advanced-entry="claudeConsole"]',
    ),
  ).toHaveCount(0);
  await expect(dialog.locator('.ol-advanced-settings-group-title')).toHaveCount(0);
  await expect(dialog.getByRole('switch')).toHaveCount(0);
  await dialog.locator('[data-ol-advanced-entry="cursorContext"]').click();
  await expect(
    dialog.getByRole('switch', { name: '光标上下文（实验）', exact: true }),
  ).toBeVisible();
  await dialog.locator('.ol-settings-back').click();
  await expect(dialog.locator('[data-ol-advanced-page="cursorContext"]')).toHaveCount(0);
  await dialog.getByRole('button', { name: '快捷键', exact: true }).click();
  await expect(dialog.getByText('Less Computer', { exact: true })).toHaveCount(0);
});
