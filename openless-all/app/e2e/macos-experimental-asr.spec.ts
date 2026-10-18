import { expect, test, type Page } from '@playwright/test';

async function settings(page: Page, os: 'mac' | 'win' = 'mac') {
  // The product OS is an explicit test input, independent of the CI runner.
  await page.addInitScript((os) => {
    const mac = os === 'mac';
    Object.defineProperty(navigator, 'platform', {
      configurable: true,
      get: () => (mac ? 'MacIntel' : 'Win32'),
    });
    Object.defineProperty(navigator, 'userAgent', {
      configurable: true,
      get: () =>
        mac
          ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)'
          : 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
    });
    Object.defineProperty(navigator, 'userAgentData', {
      configurable: true,
      get: () => ({ platform: mac ? 'macOS' : 'Windows' }),
    });
  }, os);
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
}
async function experiments(page: Page) {
  await page.getByRole('button', { name: '实验与扩展', exact: true }).click();
  const entries = page.locator('[data-ol-advanced-entry]');
  await expect(entries.first()).toHaveAttribute('data-ol-advanced-entry', 'localModels');
  await entries.first().click();
}

test('Mac removes legacy manager and exposes lazy experiments first with memory warning', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await settings(page);
  await page.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  const retired = page
    .locator('.ol-service-views')
    .getByRole('button', { name: '本地模型', exact: true });
  await expect(retired).toBeDisabled();
  await retired.locator('..').hover();
  await expect(page.getByRole('tooltip')).toHaveText('该功能已下架，请去实验性与扩展里面');
  await experiments(page);
  const stream = page.locator('[data-experimental-model="text-stream"]');
  await expect(page.locator('[data-experimental-model]').first()).toHaveAttribute(
    'data-experimental-model',
    'text-stream',
  );
  await expect(stream).toContainText('推荐 · 实时流式');
  await stream.getByRole('button', { name: /TextStream/ }).hover();
  await expect(page.getByRole('tooltip')).toContainText('localhost:7890/stream');
  await page.keyboard.press('Escape');
  const sense = page.locator('[data-experimental-model="sense-voice"]');
  const gemma = page.locator('[data-experimental-model="embedding-gemma"]');
  await sense.getByRole('button', { name: 'SenseVoice Small', exact: true }).hover();
  await expect(page.getByRole('tooltip')).toContainText('使用本地模型会增加内存占用');
  await expect(page.getByRole('tooltip')).toContainText('开始录音时才加载');
  await page.keyboard.press('Escape');
  await expect(sense.getByRole('switch')).toHaveAttribute('aria-checked', 'false');
  await stream.getByRole('switch').click();
  await sense.getByRole('switch').click();
  await gemma.getByRole('switch').click();
  const guide = page.getByRole('dialog', { name: '下载完成，还需选择语音渠道' });
  await expect(guide).toBeVisible();
  await expect(guide).toContainText('下载完成不代表已经替换当前渠道');
  // Downloads finish independently. Confirm the aggregate only after all three
  // are ready, rather than racing a legitimate second completion prompt.
  for (const name of ['TextStream', 'SenseVoice Small', 'Embedding Gemma']) {
    await expect(guide).toContainText(name);
  }
  await guide.getByRole('button', { name: '确定，前往语音渠道' }).click();
  await expect(guide).toBeHidden();
  await expect(page.getByRole('heading', { name: '语音识别渠道', exact: true })).toBeVisible();
  await expect(page.locator('[data-channel-provider="experimental-sensevoice"]')).toHaveCount(1);
  await expect(page.locator('[data-channel-provider="experimental-textstream"]')).toHaveCount(1);
  await expect(page.locator('[data-channel-provider="volcengine"]').first()).toHaveAttribute(
    'data-channel-active',
    'true',
  );
  await experiments(page);
  await sense.getByRole('switch').click();
  await expect(sense.getByRole('switch')).toHaveAttribute('aria-checked', 'false');
  await expect(sense.getByRole('button', { name: '删除', exact: true })).toBeVisible();
  await expect(gemma.getByRole('switch')).toHaveAttribute('aria-checked', 'true');
  expect(errors).toEqual([]);
});

test('Apple system recognition remains selectable without local-model controls', async ({
  page,
}) => {
  await settings(page);
  await page.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await page.getByRole('button', { name: /^语音识别。/ }).click();
  await page.getByRole('button', { name: '添加渠道', exact: true }).click();
  const form = page.getByRole('dialog', { name: '添加渠道', exact: true });
  await form.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: '苹果识别（macOS）', exact: true }).click();
  await expect(form).toContainText('Apple 语音识别使用系统内置引擎，无需选择模型。');
  await expect(form.locator('.ol-channel-local-hint')).toHaveCount(0);
  await expect(form.locator('.ol-local-model-picker')).toHaveCount(0);
  await expect(form.getByRole('textbox', { name: /API Key|Base URL/ })).toHaveCount(0);
  await form.getByRole('button', { name: '返回渠道列表', exact: true }).click();
  const card = page.locator('[data-channel-provider="apple-speech"]');
  await expect(card).toHaveCount(1);
  await expect(card).not.toHaveAttribute('data-retired-local-asr', 'true');
  const toggle = card.getByRole('switch');
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-checked', 'true');
});

test('closing settings does not auto-enable a channel or lose completion guidance', async ({
  page,
}) => {
  await settings(page);
  await experiments(page);
  await page.getByRole('switch', { name: 'SenseVoice Small', exact: true }).click();
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '返回应用', exact: true })
    .click();
  await page.waitForTimeout(2100);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const guide = page.getByRole('dialog', { name: '下载完成，还需选择语音渠道' });
  await expect(guide).toBeVisible();
  await guide.getByRole('button', { name: '稍后再说' }).click();
  await experiments(page);
  await expect(page.getByRole('switch', { name: 'SenseVoice Small', exact: true })).toHaveAttribute(
    'aria-checked',
    'true',
  );
});

test('Windows retains its model manager and does not expose the Mac experiments entry', async ({
  page,
}) => {
  await settings(page, 'win');
  await page.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await expect(
    page.locator('.ol-service-views').getByRole('button', { name: '本地模型', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await expect(page.locator('[data-ol-advanced-entry="localModels"]')).toHaveCount(0);
});
