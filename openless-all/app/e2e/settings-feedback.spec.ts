import { expect, test, type Page } from '@playwright/test';

// Development React is intentional: production builds hide special-prop/ref warnings.
test.use({ baseURL: 'http://127.0.0.1:5326' });
const messages = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  messages.set(page, errors);
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    // Chromium's software WebGL renderer reports GPU readback noise unrelated to React.
    if (message.text().includes('GL Driver Message')) return;
    if (message.type() === 'error' || message.type() === 'warning') errors.push(message.text());
  });
  await page.goto('/');
});
test.afterEach(async ({ page }) => {
  expect(messages.get(page)).toEqual([]);
});

async function settings(page: Page, category: string) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name: category, exact: true })
    .click();
}

async function expectInViewport(page: Page) {
  const bounds = (await page.getByRole('tooltip').boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.y).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(viewport.width + 1);
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(viewport.height + 1);
}

test('explanatory labels replace help badges, open beside the words and dismiss with Escape', async ({
  page,
}) => {
  await settings(page, 'AI 服务与模型');
  const label = page.getByRole('button', { name: '识别管线', exact: true });
  const row = page.locator('.ol-service-pipeline-row');
  await expect(label).toHaveCSS('text-decoration-style', 'dotted');
  await expect(label).toHaveCSS('cursor', 'pointer');
  await expect(row.locator('svg.lucide-circle-question-mark')).toHaveCount(0);
  const before = await row.boundingBox();
  await label.hover();
  const tooltip = page.getByRole('tooltip');
  await expect(tooltip).toContainText('ASR');
  await expect(row.getByRole('tooltip')).toHaveCount(0);
  await expectInViewport(page);
  expect((await tooltip.boundingBox())!.x).toBeGreaterThanOrEqual(
    (await label.boundingBox())!.x + (await label.boundingBox())!.width,
  );
  expect((await row.boundingBox())!.height).toBe(before!.height);
  expect((await row.boundingBox())!.width).toBe(before!.width);
  // Moving onto the popup must not make a multi-line explanation disappear.
  await tooltip.hover();
  await expect(tooltip).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
  await expect(page.getByRole('dialog', { name: '设置', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '传统模式', exact: true }).focus();
  await page.keyboard.press('Shift+Tab');
  await expect(label).toBeFocused();
  await expect(tooltip).toBeVisible();
  await expect(label).toHaveAttribute(
    'aria-describedby',
    (await tooltip.getAttribute('id')) as string,
  );
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
});

test('experimental models show three compact switches; memory warnings stay in the shared popup', async ({
  page,
}) => {
  await settings(page, '实验与扩展');
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  const section = page.locator('.ol-experimental-models');
  await expect(section.getByRole('switch')).toHaveCount(3);
  await expect(section.locator('.ol-channel-local-hint, p:visible')).toHaveCount(0);
  expect((await section.boundingBox())!.height).toBeLessThan(200);
  const label = section.getByRole('button', { name: 'SenseVoice Small', exact: true });
  await label.hover();
  await expect(page.getByRole('tooltip')).toContainText('240 MB');
  await expect(page.getByRole('tooltip')).toContainText('删除');
  await expect(page.getByRole('tooltip')).toContainText('使用本地模型会增加内存占用');
  await expect(section.getByRole('tooltip')).toHaveCount(0);
  await label.click();
  await expect(section.getByRole('switch', { name: 'SenseVoice Small' })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tooltip')).toBeHidden();
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  await expect(page.locator('.ol-tooltip-positioner')).toHaveCount(0);
});

for (const zoom of ['0.9', '1', '1.1']) {
  test(`font menu and tooltip remain anchored at interface zoom ${zoom}`, async ({ page }) => {
    await settings(page, '外观与语言');
    await page.getByRole('combobox', { name: '界面大小', exact: true }).click();
    await page
      .getByRole('option', { name: `${Math.round(Number(zoom) * 100)}%`, exact: true })
      .click();
    await expect(page.locator('.ol-select-popup[data-open]')).toHaveCount(0);
    const trigger = page.getByRole('combobox', { name: '字体系列', exact: true });
    await trigger.press('Enter');
    const popup = page.locator('.ol-select-popup[data-open]');
    await expect(popup).toBeVisible();
    await expect(popup).toHaveCSS('opacity', '1');
    const anchor = (await trigger.boundingBox())!;
    const bounds = (await popup.boundingBox())!;
    expect(bounds.y).toBeGreaterThanOrEqual(anchor.y + anchor.height - 1);
    expect(Math.abs(bounds.x - anchor.x)).toBeLessThan(2);
    expect(Math.abs(bounds.width - anchor.width)).toBeLessThan(2);
    await page.keyboard.press('Escape');
    await expect(popup).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.press('Enter');
    await expect(page.getByRole('option', { name: '系统默认', exact: true })).toBeFocused();
    await page.keyboard.press('End');
    await expect(page.getByRole('option', { name: 'Menlo · Consolas', exact: true })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(trigger).toContainText('Menlo');
    const label = page.getByRole('button', { name: '字体系列', exact: true });
    await label.hover();
    await expect(page.getByRole('tooltip')).toContainText('已安装字体');
    await expectInViewport(page);
    const hint = (await page.getByRole('tooltip').boundingBox())!;
    const labelBounds = (await label.boundingBox())!;
    expect(hint.x).toBeGreaterThanOrEqual(labelBounds.x + labelBounds.width);
  });
}

test('narrow windows keep select and long model explanations within the viewport', async ({
  page,
}) => {
  await settings(page, '外观与语言');
  await page.setViewportSize({ width: 375, height: 812 });
  const trigger = page.getByRole('combobox', { name: '字体系列', exact: true });
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  const bounds = (await page.locator('.ol-select-popup').boundingBox())!;
  expect(bounds.x).toBeGreaterThanOrEqual(0);
  expect(bounds.x + bounds.width).toBeLessThanOrEqual(375);
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  await page.getByRole('button', { name: 'Embedding Gemma', exact: true }).click();
  await expect(page.getByRole('tooltip')).toBeVisible();
  await expectInViewport(page);
});

test.describe('touch hints', () => {
  test.use({ hasTouch: true });
  test('a tap opens the description without changing the model switch; outside tap dismisses', async ({
    page,
  }) => {
    await settings(page, '实验与扩展');
    await page.locator('[data-ol-advanced-entry="localModels"]').click();
    await page.getByRole('button', { name: 'SenseVoice Small', exact: true }).tap();
    await expect(page.getByRole('tooltip')).toContainText('240 MB');
    // Stay open through Chromium's synthetic hover/leave events following touch.
    await page.waitForTimeout(350);
    await expect(page.getByRole('tooltip')).toBeVisible();
    await expect(page.getByRole('switch', { name: 'SenseVoice Small' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
    await page.getByRole('heading', { name: '实验性本地语音模型', exact: true }).tap();
    await expect(page.getByRole('tooltip')).toBeHidden();
  });
});

test('style card lists mount and switch without the React 18 PopChild ref warning', async ({
  page,
}) => {
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  const tabs = page.locator('main .ol-choice-chips').first().getByRole('button');
  await expect(tabs).toHaveCount(2);
  await tabs.nth(1).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
  await tabs.nth(0).click();
  await expect(page.locator('[data-ol-style-pack]').first()).toBeVisible();
});

test('Gemini Omni exposes the shared thinking setting and hides it for transcription models', async ({
  page,
}, testInfo) => {
  await settings(page, 'AI 服务与模型');
  await page.getByRole('button', { name: '多模态模式', exact: true }).click();
  const card = page.locator('.ol-omni-settings');
  await card.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: 'Google Gemini', exact: true }).click();
  const thinking = card.getByRole('switch', { name: '思考', exact: true });
  await expect(thinking).toHaveAttribute('aria-checked', 'false');
  await thinking.click();
  await expect(thinking).toHaveAttribute('aria-checked', 'true');
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const { mockSettings } = await import('/src/lib/ipc/mock-data.ts');
        return mockSettings.llmThinkingEnabled;
      }),
    )
    .toBe(true);
  await page.screenshot({ path: testInfo.outputPath('gemini-omni-desktop.png') });

  await page.setViewportSize({ width: 375, height: 812 });
  await expect(thinking).toBeInViewport({ ratio: 1 });
  const model = card.getByLabel('模型', { exact: true });
  await model.fill('gemini-3.5-transcribe');
  await model.blur();
  await expect(thinking).toHaveCount(0);
  await model.fill('gemini-3.5-flash-lite');
  await model.blur();
  await expect(thinking).toHaveAttribute('aria-checked', 'true');
  await thinking.click();
  await expect(thinking).toHaveAttribute('aria-checked', 'false');
});
