import { expect, test, type Page } from '@playwright/test';

import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';
test.use({ baseURL: fixtureOrigin });

async function openAiSettings(page: Page, labels = { settings: '设置', services: 'AI 与模型' }) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: labels.settings, exact: true }).click();
  await page.getByRole('button', { name: labels.services, exact: true }).click();
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

for (const profile of [
  {
    locale: 'zh-CN',
    scale: 'medium',
    settings: '设置',
    services: 'AI 与模型',
    modeGroup: '识别管线',
    mode: '多模态模式',
    region: '连接验证',
    hint: '手动发起文本连接请求，可能消耗服务器额度',
    fetch: '拉取模型',
    picker: '选择一个模型写入上方字段',
    verify: '验证',
    fetched: /已拉取 \d+ 个模型/,
    validated: '文本连通性通过',
    model: '模型',
  },
  {
    locale: 'en',
    scale: 'large',
    settings: 'Settings',
    services: 'AI & models',
    modeGroup: 'Pipeline mode',
    mode: 'Multimodal',
    region: 'Connection check',
    hint: 'Manually send a text-only connection request. It may use server credits.',
    fetch: 'Fetch models',
    picker: 'Select a model to fill the field above',
    verify: 'Verify',
    fetched: /Fetched \d+ models/,
    validated: 'Text connectivity passed',
    model: 'Model',
  },
]) {
  test(`multimodal tools and model menu stay usable at ${profile.locale} / ${profile.scale}`, async ({
    page,
  }) => {
    if (profile.scale === 'large') await page.setViewportSize({ width: 980, height: 640 });
    await page.addInitScript(
      ({ locale, scale }) => {
        localStorage.setItem('ol.locale', locale);
        localStorage.setItem('ol-font-scale', scale);
      },
      { locale: profile.locale, scale: profile.scale },
    );
    await openAiSettings(page, profile);
    await page
      .getByRole('group', { name: profile.modeGroup, exact: true })
      .getByRole('button', { name: profile.mode, exact: true })
      .click();
    await settleNavigation(page);
    await expect(page.locator('.ol-viewport-content')).toHaveCSS(
      'zoom',
      profile.scale === 'large' ? '1.1' : '1',
    );

    const tools = page.getByRole('region', { name: profile.region, exact: true });
    const row = tools.locator('.ol-omni-tools-actions');
    const fetch = tools.getByRole('button', { name: profile.fetch, exact: true });
    const picker = tools.getByRole('combobox', { name: profile.picker, exact: true });
    const verify = tools.getByRole('button', { name: profile.verify, exact: true });
    const hint = tools.getByText(profile.hint, { exact: true });
    const result = tools.getByRole('status');

    const expectActionRow = async (withResult = false) => {
      await expect
        .poll(async () => {
          const [actions, first, middle, last, feedback] = await Promise.all([
            row.boundingBox(),
            fetch.boundingBox(),
            picker.boundingBox(),
            verify.boundingBox(),
            withResult ? result.boundingBox() : null,
          ]);
          if (!actions || !first || !middle || !last || (withResult && !feedback)) return false;
          const centers = [first, middle, last].map((box) => box.y + box.height / 2);
          return (
            Math.max(...centers) - Math.min(...centers) <= 1 &&
            first.x + first.width <= middle.x + 1 &&
            middle.x + middle.width <= last.x + 1 &&
            last.x + last.width <= actions.x + actions.width + 1 &&
            (!feedback || feedback.y >= actions.y + actions.height - 1)
          );
        })
        .toBe(true);
    };

    await expect(hint).toBeVisible();
    await expect(hint).toHaveCSS('font-size', '11px');
    await expect(fetch).toHaveCSS('box-shadow', 'none');
    await expect(verify).toHaveCSS('box-shadow', 'none');
    await expectActionRow();
    await fetch.click();
    await expect(picker).toBeEnabled();
    await expect(result).toContainText(profile.fetched);
    await expect(hint).toBeVisible();
    await expectActionRow(true);

    await picker.click();
    const popup = page.locator('.ol-select-popup');
    await expect(popup).toBeVisible();
    expect((await popup.boundingBox())!.width).toBeGreaterThan(240);
    const option = popup.getByRole('option').first();
    await expect(option).toBeVisible();
    expect((await option.boundingBox())!.width).toBeGreaterThan(180);
    const expected = (await option.textContent())?.trim();
    expect(expected).toBeTruthy();
    await option.click();
    await expect
      .poll(async () =>
        page.evaluate(async () => {
          const providers = await import('/src/api/providers.ts');
          return providers.readCredential('omni.model', 'custom');
        }),
      )
      .toBe(expected);
    await expect(page.getByLabel(profile.model, { exact: true })).toHaveValue(expected!);
    await expect(result).toContainText(expected!);
    await expectActionRow(true);

    await verify.click();
    await expect(result).toContainText(profile.validated);
    await expectActionRow(true);
  });
}

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
