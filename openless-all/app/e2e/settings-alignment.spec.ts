import { expect, test, type Locator } from '@playwright/test';
import { openMicrophoneSettings, settleNavigation } from './support/navigation';

async function centered(group: Locator) {
  await expect(group).toBeVisible();
  for (const zoom of ['1', '1.1']) {
    await group.evaluate((node, value) => {
      node.style.zoom = value;
    }, zoom);
    for (const option of await group.getByRole('button').all()) {
      await option.click();
      await expect
        .poll(() =>
          group.evaluate((node) => {
            const selected = node
              .querySelector('button[aria-pressed="true"]')!
              .getBoundingClientRect();
            const indicator = node
              .querySelector('.ol-segmented-indicator')!
              .getBoundingClientRect();
            const track = node.getBoundingClientRect();
            return Math.max(
              Math.abs(selected.top - indicator.top),
              Math.abs(selected.left - indicator.left),
              Math.abs(selected.width - indicator.width),
              Math.abs(selected.height - indicator.height),
              Math.abs(indicator.top - track.top - (track.bottom - indicator.bottom)),
            );
          }),
        )
        .toBeLessThan(0.3);
    }
  }
}

test('shared selectors remain centered at 100 and 110 percent; awareness cards are separated', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel' });
    Object.defineProperty(navigator, 'userAgent', {
      value: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0)',
    });
    Object.defineProperty(navigator, 'userAgentData', { value: undefined });
  });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await openMicrophoneSettings(dialog);
  await centered(dialog.getByRole('group', { name: '录音方式' }));
  await page.screenshot({ path: test.info().outputPath('recording-centered.png') });
  await dialog.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
  await centered(dialog.getByRole('group', { name: '识别管线' }));
  await dialog.getByRole('button', { name: '外观与语言', exact: true }).click();
  await settleNavigation(page);
  await centered(dialog.getByRole('group', { name: '主题' }));
  await page.screenshot({ path: test.info().outputPath('theme-centered.png') });
  await dialog.getByRole('button', { name: '高级功能', exact: true }).click();
  await settleNavigation(page);
  await dialog.locator('[data-ol-advanced-entry="cursorContext"]').click();
  await settleNavigation(page);
  await expect(dialog.locator('.ol-awareness-sections')).toBeVisible();
  const gaps = await dialog.locator('.ol-awareness-sections').evaluate((node) => {
    const cards = Array.from(node.children).map((child) => child.getBoundingClientRect());
    return cards.slice(1).map((card, i) => card.top - cards[i].bottom);
  });
  expect(gaps.length).toBeGreaterThan(0);
  for (const gap of gaps) expect(gap).toBeGreaterThanOrEqual(15.5);
  await page.screenshot({ path: test.info().outputPath('awareness-separated.png') });
});
