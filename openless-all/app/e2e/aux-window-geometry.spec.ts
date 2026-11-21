import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';
import type { QaStatePayload } from '../src/api/contracts';

const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;

test.use({ reducedMotion: 'reduce' });

test('QA waits for observed geometry and reopens without the closed transcript', async ({
  page,
}) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  const host = page.locator('#native-host');
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.holdQaGeometry = true;
    bridge.applyQaGeometry(false, 320);
    bridge.emit('qa:state', { kind: 'recording', sessionId: 'old', messages: [] });
    bridge.emit('qa:state', {
      kind: 'answer',
      sessionId: 'old',
      messages: [{ role: 'assistant', content: 'Old result' }],
    } satisfies QaStatePayload);
  });
  await expect.poll(() => page.evaluate(() => (window as any).fixture.sizes.at(-1))).toBe(true);
  await expect(host).toHaveCSS('height', '80px');
  await expect(page.locator('.qa-answer-panel')).toBeHidden();

  await page.evaluate(() => (window as any).fixture.applyQaGeometry(true, 320, 220));
  const answer = page.locator('.qa-answer-panel');
  await expect(answer).toBeVisible();
  await expect(answer.getByText('Old result', { exact: true })).toBeVisible();
  await expect(host).toHaveCSS('width', '320px');
  await expect(host).toHaveCSS('height', '220px');

  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('chat-panel:closing', {});
    bridge.emit('chat-panel:shown', {});
    bridge.emit('qa:state', { kind: 'idle' } satisfies QaStatePayload);
  });
  await expect.poll(() => page.evaluate(() => (window as any).fixture.sizes.at(-1))).toBe(false);
  await expect(page.locator('.ol-default-capsule')).toBeHidden();
  await expect(answer).toHaveCount(0);
  await page.evaluate(() => (window as any).fixture.applyQaGeometry(false, 320));
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
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
      messages: [{ role: 'assistant', content: 'Answer' }],
    } satisfies QaStatePayload);
  });
  const alert = page.locator('.qa-layout-failure');
  await expect(alert).toBeVisible();
  await expect(page.locator('#native-host')).toHaveCSS('height', '80px');
  const close = alert.getByRole('button');
  await expect(close).toBeInViewport();
  await close.click();
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

test('the fallback card starts its entrance and exposes Copy only after its presentation fits', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' });
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
  await expect(waiting.locator('button').first()).toBeDisabled();
  await expect(waiting.locator(':scope > div')).toHaveCSS('animation-name', 'none');
  await page.evaluate(() => {
    document.addEventListener('animationstart', (event) => {
      if (event.animationName !== 'capsule-in' || !(event.target instanceof HTMLElement)) return;
      const layout = event.target.closest<HTMLElement>('[data-layout]')?.dataset.layout;
      if (layout) document.body.dataset.fallbackAnimationStart = layout;
    });
  });
  await page.setViewportSize({ width: 360, height: report.height + 1 });
  const ready = page.locator('[data-layout=ready]');
  await expect(ready).toBeVisible();
  await expect(page.locator('body')).toHaveAttribute('data-fallback-animation-start', 'ready');
  await ready.getByRole('button').first().click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.copiedFallback.at(-1)))
    .toBe('待复制的长文本。\n'.repeat(36));
});

test('a bounded fallback stays usable without motion when the host never expands', async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 112 });
  await page.goto(fixture + '?fallback');
  const bounded = page.locator('[data-layout=bounded]');
  await expect(bounded).toBeVisible();
  await expect(bounded.locator(':scope > div')).toHaveCSS('animation-name', 'none');
  await expect(bounded.getByRole('button').first()).toBeInViewport({ ratio: 1 });
  await expect(bounded.getByRole('button').last()).toBeInViewport({ ratio: 1 });
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.fallbackReports.length))
    .toBe(1);
});
