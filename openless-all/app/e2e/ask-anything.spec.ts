import { expect, test, type Page } from '@playwright/test';
const fixture = 'http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html';
async function emit(page: Page, event: string, payload: unknown) {
  await page.evaluate((v) => (window as any).fixture.emit(v.event, v.payload), { event, payload });
}

test('translation uses the shared popup and native headroom; save failure preserves the old language', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 460, height: 172 });
  await page.exposeFunction('resizeMenuForTest', async (open: boolean) => {
    await page.setViewportSize({ width: 460, height: open ? 492 : 172 });
  });
  await page.goto(fixture);
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate(() => (window as any).fixture.setCapsule({ translation: true }));
  const picker = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(picker).toBeEnabled();
  await picker.click();
  const popup = page.locator('.ol-select-popup');
  await expect(popup).toBeVisible();
  await expect(popup).toHaveCSS('border-radius', '10px');
  const bounds = await popup.boundingBox();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.height).toBeGreaterThan(200);
  await page.screenshot({ path: info.outputPath('translation-menu.png') });
  await page.getByRole('option', { name: '日语', exact: true }).click();
  await expect(picker).toContainText('日语');
  await expect(popup).toBeHidden();
  await expect.poll(() => page.viewportSize()?.height).toBe(172);
  expect(await page.evaluate(() => (window as any).fixture.targetChanges.at(-1))).toEqual({
    sessionId: 'recording-a',
    targetLanguage: '日本語',
  });
  await page.evaluate(() => ((window as any).fixture.failTarget = true));
  await picker.click();
  await page.getByRole('option', { name: '法语', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('语言切换失败');
  await expect(picker).toContainText('日语');
  await expect(picker).toBeEnabled();
});

test('translation popup closes when the recording owner changes', async ({ page }) => {
  await page.goto(fixture);
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate(() => (window as any).fixture.setCapsule({ translation: true }));
  const picker = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(picker).toBeEnabled();
  await picker.click();
  await expect(page.locator('.ol-select-popup')).toBeVisible();
  await page.evaluate(() => {
    const f = (window as any).fixture;
    f.target.sessionId = 'recording-b';
    f.emit('capsule:state', { state: 'recording', sessionId: 'recording-b', translation: true });
  });
  await expect(page.locator('.ol-select-popup')).toBeHidden();
  expect(await page.evaluate(() => (window as any).fixture.targetChanges)).toEqual([]);
});

test('Ask has no desktop composer; provider tool status stays in the capsule until the final answer', async ({
  page,
}, info) => {
  await page.goto(fixture + '?qa');
  await page.locator('.ol-default-capsule').waitFor();
  await expect(page.locator('input')).toHaveCount(0);
  await emit(page, 'qa:state', {
    kind: 'recording',
    sessionId: 'ask-a',
    messages: [],
    selectionPreview: 'selected source',
  });
  await expect(page.locator('.qa-voice-hint')).toContainText('selected source');
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'ask-a', messages: [] });
  await emit(page, 'qa:state', {
    kind: 'activity',
    sessionId: 'ask-a',
    activity: { kind: 'searching_web', toolName: 'web_search' },
  });
  await expect(page.locator('.ol-default-status')).toContainText('正在搜索网页');
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'ask-a', chunk: 'partial' });
  await expect(page.locator('.qa-answer-panel')).toHaveCount(0);
  await emit(page, 'qa:state', {
    kind: 'activity',
    sessionId: 'old-ask',
    activity: { kind: 'using_tool', toolName: 'stale' },
  });
  await expect(page.locator('.ol-default-status')).toContainText('正在搜索网页');
  await emit(page, 'qa:state', {
    kind: 'answer',
    sessionId: 'ask-a',
    messages: [
      { role: 'user', content: '问题', selectionText: 'selected source' },
      { role: 'assistant', content: '最终回答' },
    ],
  });
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(page.locator('.qa-assistant-message')).toContainText('最终回答');
  await expect(page.locator('.qa-follow-up')).toHaveCount(0);
  await expect(page.locator('input')).toHaveCount(0);
  await page.screenshot({ path: info.outputPath('ask-result.png') });
});

test('overview is restored and the Ask page remains without the added three-row guide', async ({
  page,
}, info) => {
  await page.goto('/');
  await expect(page.locator('[data-ol-page="overview"]')).toBeVisible();
  await expect(page.locator('.ol-recording-key-guide')).toHaveCount(0);
  await page.screenshot({ path: info.outputPath('overview-restored.png') });
  const nav = page.locator('.ol-sidebar-surface nav');
  await nav.getByRole('button', { name: '工具', exact: true }).click();
  await nav.getByRole('button', { name: '问任何问题', exact: true }).click();
  await expect(page.getByText('提问附加键', { exact: true })).toBeVisible();
  await expect(page.locator('.ol-recording-key-guide')).toHaveCount(0);
  await expect(page.getByText('保存历史', { exact: true })).toBeVisible();
});
