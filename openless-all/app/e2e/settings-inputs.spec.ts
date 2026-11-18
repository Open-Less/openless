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

test('multimodal tools share a flat row and the model menu stays readable', async ({ page }) => {
  await openAiSettings(page);
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  const tools = page.getByRole('region', { name: '连接验证' });
  const fetch = tools.getByRole('button', { name: '拉取模型', exact: true });
  const picker = tools.getByRole('combobox', { name: '选择一个模型写入上方字段' });
  const verify = tools.getByRole('button', { name: '验证', exact: true });
  const result = tools.getByRole('status');
  await expect(tools.getByText('手动发起文本连接请求，可能消耗服务器额度')).toBeVisible();
  await expect(fetch).toHaveCSS('box-shadow', 'none');
  await expect(verify).toHaveCSS('box-shadow', 'none');
  await fetch.click();
  await expect(picker).toBeEnabled();
  await expect(result).toContainText(/已拉取 \d+ 个模型/);
  await expect
    .poll(async () => {
      const bounds = await Promise.all([fetch, result, picker, verify].map((node) => node.boundingBox()));
      if (bounds.some((box) => box === null)) return false;
      const items = bounds.map((box) => box!);
      const centers = items.map((box) => box.y + box.height / 2);
      return (
        Math.max(...centers) - Math.min(...centers) <= 1 &&
        items.every((box, index) => index === 0 || items[index - 1].x < box.x)
      );
    })
    .toBe(true);
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
  await verify.click();
  await expect(result).toContainText('文本连通性通过');
});

test('multimodal provider switches flush the outgoing field to its own slot', async ({ page }) => {
  await openAiSettings(page);
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  await settleNavigation(page);
  const card = page.locator('.ol-omni-settings');
  const provider = card.getByRole('combobox', { name: '供应商', exact: true });
  const model = card.getByLabel('模型', { exact: true });
  await expect(provider).toHaveText('自定义');
  await model.click();
  await model.fill('saved-with-custom-provider');
  await expect(model).toHaveValue('saved-with-custom-provider');
  await provider.click();
  await page.getByRole('option', { name: 'Google Gemini', exact: true }).click();
  await expect(provider).toContainText('Google Gemini');
  await expect(model).toHaveValue('gemini-3.5-flash-lite');
  const customModel = await page.evaluate(async () => {
    const { readCredential } = await import('/src/api/providers.ts');
    return readCredential('omni.model', 'custom');
  });
  expect(customModel).toBe('saved-with-custom-provider');
  await provider.click();
  await page.getByRole('option', { name: '自定义', exact: true }).click();
  await expect(model).toHaveValue('saved-with-custom-provider');
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
