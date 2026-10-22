import { expect, test, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';

const fixture = 'http://127.0.0.1:5326/e2e/fixtures/refinements.html';
async function openSettings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  return page.getByRole('dialog', { name: '设置', exact: true });
}

test('110% is the fresh default; explicit 100% survives reload and the settings titlebar stays draggable', async ({
  page,
}) => {
  const dialog = await openSettings(page);
  await expect(page.locator('html')).toHaveCSS('zoom', '1.1');
  await expect(dialog.getByRole('button', { name: '返回应用', exact: true })).toHaveCount(1);
  await expect(dialog.getByRole('button', { name: '关闭', exact: true })).toHaveCount(0);
  expect(
    await page.evaluate(
      () => !!document.elementFromPoint(200, 20)?.closest('[data-tauri-drag-region]'),
    ),
  ).toBe(true);
  await dialog.getByRole('button', { name: '外观与语言', exact: true }).click();
  const interfaceSize = page.getByRole('combobox', { name: '界面大小', exact: true });
  await expect(interfaceSize).toContainText('110%');
  await interfaceSize.click();
  await page.getByRole('option', { name: '100%', exact: true }).click();
  await expect(page.locator('html')).toHaveCSS('zoom', '1');
  await page.reload();
  await expect(page.locator('html')).toHaveCSS('zoom', '1');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  expect(
    await page.evaluate(
      () => !!document.elementFromPoint(200, 20)?.closest('[data-tauri-drag-region]'),
    ),
  ).toBe(true);
});

test('returning to recording at 100% has stable control geometry and one shortcut recording entry', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const dialog = await openSettings(page);
  for (let attempt = 0; attempt < 3; attempt++) {
    await dialog.getByRole('button', { name: '外观与语言', exact: true }).click();
    await expect(page.locator('.ol-typography')).toBeVisible();
    await dialog.getByRole('button', { name: '录音与输入', exact: true }).click();
    const key = dialog.locator('.ol-shortcut-binding').first();
    await expect(key).toBeVisible();
    const positions = await key.evaluate(async (element) => {
      const values: number[] = [];
      for (let frame = 0; frame < 18; frame++) {
        await new Promise(requestAnimationFrame);
        values.push(element.getBoundingClientRect().y);
      }
      return values;
    });
    expect(Math.max(...positions) - Math.min(...positions)).toBeLessThan(1);
  }
  const recorder = dialog.locator('.ol-shortcut-recorder').first();
  await expect(recorder.locator('.ol-shortcut-menu')).toHaveCount(0);
  await recorder.locator('.ol-shortcut-binding').click();
  await expect(page.locator('.ol-shortcut-recording')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(recorder.locator('.ol-shortcut-binding')).toBeVisible();
  await expect(dialog).toBeVisible();
  await recorder.locator('.ol-shortcut-binding').click();
  await page.keyboard.press('F8');
  await expect(recorder.locator('.ol-shortcut-binding')).toContainText('F8');
  await expect(page.locator('.ol-shortcut-recording')).toBeHidden();
  expect(errors).toEqual([]);
});

for (const scale of [1, 1.1]) {
  test(`long official menus keep user scroll through live option updates at ${Math.round(scale * 100)}%`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1100, height: 1200 });
    await page.goto(fixture);
    await page.evaluate((scale) => {
      document.documentElement.style.zoom = String(scale);
      document.documentElement.style.setProperty('--ol-interface-scale', String(scale));
    }, scale);
    const trigger = page.getByRole('combobox', { name: 'Long menu', exact: true });
    await trigger.click();
    const popup = page.locator('.ol-select-popup');
    await expect(popup).toBeVisible();
    const anchor = (await trigger.boundingBox())!;
    const bounds = (await popup.boundingBox())!;
    expect(bounds.y).toBeGreaterThanOrEqual(anchor.y + anchor.height - 1);
    expect(Math.abs(bounds.x - anchor.x)).toBeLessThan(2);
    const list = page.locator('.ol-select-list');
    await list.evaluate((element) => {
      element.scrollTop = element.scrollHeight;
    });
    const bottom = await list.evaluate((element) => element.scrollTop);
    expect(bottom).toBeGreaterThan(500);
    // Five live meter-like updates used to reset highlight and force the list to its top.
    await page.waitForTimeout(650);
    expect(Math.abs((await list.evaluate((element) => element.scrollTop)) - bottom)).toBeLessThan(
      2,
    );
    await page.getByRole('option', { name: /^Option 59/ }).click();
    await expect(trigger).toContainText('Option 59');
    const searchable = page.getByRole('combobox', { name: 'Searchable menu', exact: true });
    await searchable.click();
    await page.getByRole('combobox', { name: 'Search options', exact: true }).fill('Option 58');
    await expect(page.getByRole('option')).toHaveCount(1);
    await page.getByRole('option', { name: /^Option 58/ }).click();
    await expect(searchable).toContainText('Option 58');
  });
}

test('experimental download cancel and switch-off share state, do not reappear, and do not push later rows', async ({
  page,
}) => {
  await openSettings(page);
  await page.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  const sense = page.locator('[data-experimental-model="sense-voice"]');
  const next = page.locator('[data-experimental-model="embedding-gemma"]');
  const toggle = sense.getByRole('switch');
  // Native WebViews retain CSS from previously visited model pages. Load it
  // late, as on device, rather than testing only the first clean browser entry.
  await page.addStyleTag({
    content: readFileSync(new URL('../src/pages/LocalAsr/local-asr.css', import.meta.url), 'utf8'),
  });
  await expect(toggle).toBeEnabled();
  const before = (await next.boundingBox())!.y;
  await toggle.click();
  const card = page.locator('.ol-download-card[data-download-id="sense-voice"]');
  await expect(card).toBeVisible();
  expect(Math.abs((await next.boundingBox())!.y - before)).toBeLessThan(1);
  const statusText = await sense.locator('.ol-experimental-model-status').evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    const text = range.getBoundingClientRect();
    const progress = element.parentElement!.querySelector('progress')!.getBoundingClientRect();
    return { right: text.right, progressRight: progress.right };
  });
  expect(Math.abs(statusText.right - statusText.progressRight)).toBeLessThan(2);
  await card.getByRole('button', { name: '取消', exact: true }).click();
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
  await expect(card).toHaveCount(0);
  await toggle.click();
  await expect(card).toBeVisible();
  await toggle.click();
  await expect(card).toHaveCount(0);
  await page.waitForTimeout(2100);
  await expect(card).toHaveCount(0);
  await expect(page.getByRole('dialog', { name: '下载完成，还需选择语音渠道' })).toHaveCount(0);
  await expect(toggle).toHaveAttribute('aria-checked', 'false');
});

test('shared modal Escape retains an inert exit and restores focus', async ({ page }) => {
  await page.goto(fixture);
  const open = page.getByRole('button', { name: 'Open modal fixture', exact: true });
  // Safari pointer clicks intentionally need not focus a button. Verify return
  // to an actual keyboard opener rather than assuming pointer focus semantics.
  await open.focus();
  await open.press('Enter');
  await expect(page.getByRole('dialog', { name: 'Modal fixture', exact: true })).toBeVisible();
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (...args) {
      const animation = animate.apply(this, args);
      if (this instanceof HTMLElement && this.matches('.ol-dialog-overlay')) {
        queueMicrotask(() => {
          if (animation.id === 'ol-surface-exit') {
            const card = document.querySelector<HTMLElement>('.ol-dialog-card');
            document.body.dataset.modalExit = JSON.stringify({
              present: !!card,
              inert: card?.inert,
            });
          }
        });
      }
      return animation;
    };
  });
  await page.keyboard.press('Escape');
  await expect(page.locator('body')).toHaveAttribute(
    'data-modal-exit',
    JSON.stringify({ present: true, inert: true }),
  );
  await expect(page.locator('.ol-dialog-overlay')).toHaveCount(0);
  await expect(open).toBeFocused();
});

test('experimental completion guidance consumes Escape without closing settings', async ({
  page,
}) => {
  const settings = await openSettings(page);
  await settings.getByRole('button', { name: '实验与扩展', exact: true }).click();
  await page.locator('[data-ol-advanced-entry="localModels"]').click();
  await page.getByRole('switch', { name: 'SenseVoice Small', exact: true }).click();
  const guide = page.getByRole('dialog', { name: '下载完成，还需选择语音渠道', exact: true });
  await expect(guide).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(guide).toHaveCount(0);
  await expect(settings).toBeVisible();
  await expect(page.getByRole('switch', { name: 'SenseVoice Small', exact: true })).toHaveAttribute(
    'aria-checked',
    'true',
  );
});

test('conditional notices animate actual layout downward and retain content during exit', async ({
  page,
}) => {
  await page.goto(fixture);
  const following = page.getByTestId('following');
  await following.scrollIntoViewIfNeeded();
  const initial = (await following.boundingBox())!.y;
  await page.getByRole('button', { name: 'Toggle notice', exact: true }).click();
  const positions = await following.evaluate(async (element) => {
    const values: number[] = [];
    for (let i = 0; i < 20; i++) {
      await new Promise(requestAnimationFrame);
      values.push(element.getBoundingClientRect().y);
    }
    return values;
  });
  const final = positions.at(-1)!;
  expect(final - initial).toBeGreaterThan(90);
  expect(positions.some((value) => value > initial + 1 && value < final - 1)).toBe(true);
  await page.getByRole('button', { name: 'Toggle notice', exact: true }).click();
  await expect(page.getByTestId('notice')).toHaveText('Working status');
  await expect(page.getByTestId('notice')).toBeHidden();
});

test('voice activation opens model setup without leaving the multimodal pipeline', async ({
  page,
}, testInfo) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'platform', { get: () => 'Win32' });
    Object.defineProperty(navigator, 'userAgent', { get: () => 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' });
  });
  await page.goto('http://127.0.0.1:5326/?os=win');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  const categories = dialog.getByRole('navigation', { name: '设置分类' });
  await categories.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).click();
  const models = dialog
    .locator('.ol-service-views')
    .getByRole('button', { name: /^本地模型/ });
  await expect(models).toBeDisabled();
  await categories.getByRole('button', { name: '录音与输入', exact: true }).click();
  await dialog.getByRole('switch', { name: '语音唤醒听写', exact: true }).click();
  const download = dialog.getByRole('button', {
    name: '前往 AI 服务与模型下载 Zipformer',
    exact: true,
  });
  await expect(download).toBeVisible();
  const providerState = () =>
    page.evaluate(async () => {
      const { mockSettings } = await import('/src/lib/ipc/mock-data.ts');
      return [
        mockSettings.pipelineMode,
        mockSettings.activeAsrProvider,
        mockSettings.activeOmniProvider,
      ];
    });
  const before = await providerState();
  await download.focus();
  await page.keyboard.press('Enter');
  await expect(models).toHaveAttribute('aria-pressed', 'true');
  await expect(dialog.getByRole('button', { name: '多模态模式', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(
    dialog.getByText('此处准备语音唤醒模型，听写仍使用当前多模态服务。', { exact: true }),
  ).toBeVisible();
  await expect(
    dialog.locator('.ol-service-views').getByRole('button', { name: /^语言模型/ }),
  ).toBeDisabled();
  await expect(
    dialog.locator('.ol-service-views').getByRole('button', { name: /^语音识别/ }),
  ).toBeDisabled();
  expect(await providerState()).toEqual(before);
  await page.screenshot({ path: testInfo.outputPath('voice-activation-model-setup.png') });
  await categories.getByRole('button', { name: '录音与输入', exact: true }).click();
  await dialog.getByRole('switch', { name: '语音唤醒听写', exact: true }).click();
  await categories.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await expect(models).toBeDisabled();
});
