import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';

test('an opaque destination still covers the retained service tabs', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
  await page.getByRole('button', { name: '传统模式', exact: true }).click();
  await settleNavigation(page);
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (keyframes, options) {
      const animation = animate.call(this, keyframes, options);
      if (this.matches('.ol-transition-page-with-header[data-transition-phase="enter"]')) {
        Element.prototype.animate = animate;
        animation.pause();
        animation.currentTime = Number(animation.effect!.getComputedTiming().activeDuration) / 2;
      }
      return animation;
    };
  });
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  const incoming = page.locator('.ol-transition-page-with-header[data-transition-phase="enter"]');
  await expect(incoming).toHaveCount(1);
  const covered = await incoming.evaluate((node) => {
    // Exercise the exact completion pose while React still retains the outgoing page:
    // opacity=1 and no animation-generated stacking context.
    const animation = node.getAnimations().find((entry) => entry.id === 'ol-page-enter')!;
    (node as HTMLElement).style.opacity = '1';
    (node as HTMLElement).style.willChange = '';
    animation.cancel();
    const oldNested = document.querySelector('.ol-service-views')!;
    const rect = oldNested.getBoundingClientRect();
    const hits = document.elementsFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    // Both surfaces are inert during handoff, so enable hit testing for this paint-order probe.
    for (const surface of document.querySelectorAll<HTMLElement>(
      '.ol-transition-page-with-header',
    )) {
      surface.inert = false;
      surface.style.pointerEvents = 'auto';
    }
    const top = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
    return { covered: node.contains(top), top: top?.outerHTML.slice(0, 250), hits: hits.length };
  });
  expect(covered.covered, JSON.stringify(covered)).toBe(true);
});

declare global {
  interface Window {
    microphoneEntryFixture: import('./fixtures/microphone-entry').MicrophoneEntryFixture;
  }
}
const microphoneFixture = `${fixtureOrigin}/e2e/fixtures/microphone-entry.html`;
async function microphoneCalls(page: import('@playwright/test').Page, command: string) {
  return page.evaluate(
    (name) => window.microphoneEntryFixture.calls.filter((call) => call === name).length,
    command,
  );
}
async function openMicrophone(page: import('@playwright/test').Page) {
  await page.locator('[data-ol-general-entry="recording"]').click();
  await settleNavigation(page);
}

test('microphone entry waits for capabilities and reads existing permission without requesting it', async ({
  page,
}) => {
  await page.goto(microphoneFixture);
  await openMicrophone(page);
  expect(await microphoneCalls(page, 'check_microphone_permission')).toBe(0);
  expect(await microphoneCalls(page, 'list_microphone_devices')).toBe(0);
  await page.evaluate(() => window.microphoneEntryFixture.releaseCapabilities());
  await expect.poll(() => microphoneCalls(page, 'list_microphone_devices')).toBe(1);
  await expect(
    page.getByRole('combobox', { name: 'Preferred microphone', exact: true }),
  ).toBeVisible();
  expect(await microphoneCalls(page, 'check_microphone_permission')).toBe(1);
  expect(await microphoneCalls(page, 'request_microphone_permission')).toBe(0);
  await page.evaluate(() => {
    window.dispatchEvent(new Event('focus'));
    document.dispatchEvent(new Event('visibilitychange'));
  });
  expect(await microphoneCalls(page, 'check_microphone_permission')).toBe(1);
});

test('microphone entry rechecks only after explicit system settings and ignores a retired permission read', async ({
  page,
}) => {
  await page.goto(microphoneFixture);
  await page.evaluate(() => {
    window.microphoneEntryFixture.setStatus('denied');
    window.microphoneEntryFixture.releaseCapabilities();
  });
  await openMicrophone(page);
  const openSystem = page.getByRole('button', { name: 'Open System Settings', exact: true });
  await expect(openSystem).toBeVisible();
  expect(await microphoneCalls(page, 'list_microphone_devices')).toBe(0);
  await openSystem.click();
  expect(await microphoneCalls(page, 'open_system_settings')).toBe(1);
  const before = await microphoneCalls(page, 'check_microphone_permission');
  await page.evaluate(() => {
    window.microphoneEntryFixture.setStatus('granted');
    window.dispatchEvent(new Event('focus'));
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect(
    page.getByRole('combobox', { name: 'Preferred microphone', exact: true }),
  ).toBeVisible();
  expect(await microphoneCalls(page, 'check_microphone_permission')).toBe(before + 1);
  expect(await microphoneCalls(page, 'request_microphone_permission')).toBe(0);
  await page.keyboard.press('Escape');
  await settleNavigation(page);
  await page.evaluate(() => {
    window.microphoneEntryFixture.setStatus('denied');
    window.microphoneEntryFixture.deferPermission();
  });
  await openMicrophone(page);
  await page.keyboard.press('Escape');
  await settleNavigation(page);
  await page.evaluate(() => window.microphoneEntryFixture.setStatus('granted'));
  await openMicrophone(page);
  await expect(
    page.getByRole('combobox', { name: 'Preferred microphone', exact: true }),
  ).toBeVisible();
  await page.evaluate(() => window.microphoneEntryFixture.releasePermission());
  await expect(openSystem).toHaveCount(0);
  await expect(
    page.getByRole('combobox', { name: 'Preferred microphone', exact: true }),
  ).toBeVisible();
});
