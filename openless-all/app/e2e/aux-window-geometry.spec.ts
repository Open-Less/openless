import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';
import type { QaStatePayload } from '../src/api/contracts';

const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;

test.use({ reducedMotion: 'reduce' });

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
