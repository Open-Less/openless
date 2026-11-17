import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';

const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;

test.use({ reducedMotion: 'reduce' });

test('QA waits for observed geometry after acknowledgment and discards a closing layout', async ({
  page,
}) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  const host = page.locator('#native-host');
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.holdQaGeometry = true;
    bridge.emit('qa:state', { kind: 'recording', sessionId: 'old', messages: [] });
    bridge.emit('qa:state', {
      kind: 'answer',
      sessionId: 'old',
      messages: [{ role: 'assistant', text: 'Old result' }],
    });
  });
  await expect.poll(() => page.evaluate(() => (window as any).fixture.sizes.at(-1))).toBe(true);
  await expect(host).toHaveCSS('height', '80px');
  await expect(page.locator('.qa-answer-panel')).toBeHidden();

  await page.evaluate(() => (window as any).fixture.applyQaGeometry(true, 320, 220));
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(host).toHaveCSS('height', '220px');

  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('chat-panel:closing', {});
    bridge.emit('chat-panel:shown', {});
    bridge.emit('qa:state', { kind: 'recording', sessionId: 'new', messages: [] });
  });
  await expect.poll(() => page.evaluate(() => (window as any).fixture.sizes.at(-1))).toBe(false);
  await expect(page.locator('.ol-default-capsule')).toBeHidden();
  await page.evaluate(() => (window as any).fixture.applyQaGeometry(false));
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await expect(page.locator('.qa-answer-panel')).toHaveCount(0);
});

test('QA keeps a reachable close action when the host rejects expansion', async ({ page }) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.rejectQaExpansion = true;
    bridge.emit('qa:state', { kind: 'recording', sessionId: 'failed', messages: [] });
    bridge.emit('qa:state', {
      kind: 'answer',
      sessionId: 'failed',
      messages: [{ role: 'assistant', text: 'Answer' }],
    });
  });
  const alert = page.locator('.qa-layout-failure');
  await expect(alert).toBeVisible();
  await expect(page.locator('#native-host')).toHaveCSS('height', '80px');
  await alert.getByRole('button').click();
  const closing = page.locator('.qa-capsule-shell.is-closing');
  await expect(closing).toHaveAttribute('inert', '');
  await expect(closing).toHaveCSS('opacity', '0');
});

test('language menu waits for drawable room and releases its own reservation on Escape', async ({
  page,
}) => {
  await page.setViewportSize({ width: 460, height: 92 });
  await page.goto(fixture);
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await page.evaluate(() => (window as any).fixture.setCapsule({ translation: true }));
  const picker = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(picker).toBeEnabled();
  await picker.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.menuChanges.at(-1)?.open))
    .toBe(true);
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.menuChanges.at(-1)?.open))
    .toBe(false);
  await picker.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.menuChanges.at(-1)?.open))
    .toBe(true);
  await page.setViewportSize({ width: 460, height: 415 });
  const list = page.getByRole('listbox');
  await expect(list).toBeVisible();
  expect((await list.boundingBox())!.height).toBeGreaterThan(150);
  await page.keyboard.press('Escape');
  await expect(list).toHaveCount(0);
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.menuChanges.at(-1)))
    .toEqual({
      open: false,
      sessionId: 'recording-a',
    });
});

test('the fallback card exposes Copy only after its presentation fits', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 112 });
  await page.goto(fixture + '?fallback');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.fallbackReports.length))
    .toBe(1);
  const report = await page.evaluate(() => (window as any).fixture.fallbackReports[0]);
  expect(report.presentationId).toBe(23);
  expect(report.height).toBeGreaterThan(112);
  const waiting = page.locator('[data-layout=waiting]');
  await expect(waiting).toHaveCSS('visibility', 'hidden');
  await page.setViewportSize({ width: 360, height: report.height + 1 });
  const ready = page.locator('[data-layout=ready]');
  await expect(ready).toBeVisible();
  await ready.getByRole('button').first().click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.copiedFallback.at(-1)))
    .toBe('待复制的长文本。\n'.repeat(36));
});
