import { expect, test, type Page } from '@playwright/test';

test.use({ baseURL: 'http://127.0.0.1:5326' });

async function openAiSettings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await page.waitForTimeout(300);
}

test('channel fields stay editable right after the editor opens', async ({ page }) => {
  await openAiSettings(page);
  await page.locator('.ol-channel-edit-button').first().click();
  const dialog = page.locator('.ol-channel-dialog');
  await expect(dialog).toBeVisible();

  const name = dialog.locator('input[placeholder*="例如"]').first();
  const apiKey = dialog.locator('input[type="password"]').first();
  const endpoint = dialog.locator('input[placeholder*="https://"]').first();
  const model = dialog.locator('input[placeholder*="模型"], input[placeholder*="Qwen"]').first();

  for (const [field, value] of [
    [name, '改名测试'],
    [apiKey, 'sk-input-test'],
    [endpoint, 'https://example.test/v1'],
    [model, 'model-input-test'],
  ] as const) {
    await expect(field).toBeEditable();
    await field.click();
    await field.fill(value);
    await expect(field).toHaveValue(value);
  }
});

test('multimodal model menu stays wide enough to read every option', async ({ page }) => {
  await openAiSettings(page);
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  const fetch = page.getByRole('button', { name: '拉取模型' });
  if (await fetch.count()) {
    await fetch.click();
    await page.waitForTimeout(800);
  }
  const picker = page.getByRole('combobox', { name: '选择一个模型写入上方字段' });
  if (await picker.isDisabled()) return;
  await picker.click();
  const popup = page.locator('.ol-select-popup');
  await expect(popup).toBeVisible();
  expect((await popup.boundingBox())!.width).toBeGreaterThan(240);
  const option = page.getByRole('option').first();
  await expect(option).toBeVisible();
  expect((await option.boundingBox())!.width).toBeGreaterThan(180);
});
