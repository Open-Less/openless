import { fixtureOrigin } from './support/server';
import { expect, test, type Page } from '@playwright/test';
const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;
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
  await expect(popup).toHaveClass(/ol-capsule-language-popup/);
  await expect(popup).toHaveCSS('backdrop-filter', 'none');
  await popup.evaluate((node) => node.setAttribute('data-test-retained', 'true'));
  for (let i = 0; i < 8; i += 1) {
    await page.evaluate((level) => (window as any).fixture.setCapsule({ level }), i / 10);
    await expect(popup).toHaveAttribute('data-test-retained', 'true');
    await expect(popup).toHaveCSS('opacity', '1');
  }
  const clipped = await popup.evaluate((node) =>
    getComputedStyle(node).clipPath.startsWith('shape('),
  );
  await expect(popup).toHaveCSS('border-radius', clipped ? '0px' : '16px');
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

test('Ask history lives in General instead of the tools menu', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const history = page.locator('[data-ol-general-entry="askHistory"]');
  await expect(page.locator('[data-ol-general-entry="recording"] + [data-ol-general-entry="askHistory"]')).toBeVisible();
  await history.click();
  await expect(page.getByRole('switch', { name: '保存历史' })).toBeVisible();
  await expect(page.getByText('开启后在此设备上保存「随便问」的历史记录，默认关闭。', { exact: true }).first()).toBeVisible();
});
