import { expect, test, type Page } from '@playwright/test';

import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';
test.use({ baseURL: fixtureOrigin });

async function openAiSettings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
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
  await fetch.click();
  const picker = page.getByRole('combobox', { name: '选择一个模型写入上方字段' });
  await expect(picker).toBeEnabled();
  await picker.click();
  const popup = page.locator('.ol-select-popup');
  await expect(popup).toBeVisible();
  expect((await popup.boundingBox())!.width).toBeGreaterThan(240);
  const option = page.getByRole('option').first();
  await expect(option).toBeVisible();
  expect((await option.boundingBox())!.width).toBeGreaterThan(180);
  const selected = await option.textContent();
  await option.click();
  const expected = selected?.trim();
  expect(expected).toBeTruthy();
  await expect
    .poll(async () =>
      page.evaluate(async () => {
        const providers = await import('/src/api/providers.ts');
        return providers.readCredential('omni.model', 'custom');
      }),
    )
    .toBe(expected);
  await expect(page.getByLabel('模型', { exact: true })).toHaveValue(expected!);
});

test('multimodal provider switches flush the outgoing field to its own slot', async ({ page }) => {
  await openAiSettings(page);
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  const card = page.locator('.ol-omni-settings');
  const provider = card.getByRole('combobox', { name: '供应商' });
  const model = card.getByLabel('模型', { exact: true });
  await model.fill('saved-with-custom-provider');
  await provider.click();
  await page.getByRole('option', { name: 'Google Gemini', exact: true }).click();
  await expect(provider).toContainText('Google Gemini');
  await expect(model).not.toHaveValue('saved-with-custom-provider');
  const customModel = await page.evaluate(async () => {
    const { readCredential } = await import('/src/api/providers.ts');
    return readCredential('omni.model', 'custom');
  });
  expect(customModel).toBe('saved-with-custom-provider');
});

test('Escape from a General subpage returns focus to its category', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const category = page.getByRole('button', { name: /麦克风与录音/ });
  await category.click();
  const heading = page.getByRole('heading', { name: '麦克风与录音', exact: true });
  await expect(heading).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(category).toBeFocused();
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
});
