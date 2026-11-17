import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';
test.use({ baseURL: fixtureOrigin });
test('sources sit left, actions sit below the article on the right, and only suite mode offers read aloud', async ({
  page,
}) => {
  await page.route('https://icons.duckduckgo.com/**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'image/svg+xml',
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="blue"/></svg>',
    }),
  );
  await page.goto('/e2e/fixtures/qa-footer.html');
  const read = page.getByRole('button', { name: 'DeepSeek 朗读', exact: true });
  await expect(read).toHaveCount(0);
  await page.getByRole('button', { name: 'multimodal', exact: true }).click();
  await expect(read).toHaveCount(0);
  await page.getByRole('button', { name: 'deepseek', exact: true }).click();
  await expect(read).toBeVisible();
  const footer = page.locator('.qa-answer-footer');
  await expect(footer.locator('img')).toHaveCount(2);
  await expect
    .poll(() =>
      footer
        .locator('img')
        .evaluateAll((images) =>
          images.every((image) => (image as HTMLImageElement).naturalWidth > 0),
        ),
    )
    .toBe(true);
  const boxes = await page.evaluate(() => {
    const rect = (selector: string) =>
      document.querySelector(selector)!.getBoundingClientRect().toJSON();
    return {
      paragraph: rect('article p:last-of-type'),
      footer: rect('.qa-answer-footer'),
      sources: rect('.qa-answer-sources'),
      copy: rect('.qa-answer-actions button:first-child'),
      read: rect('.qa-answer-actions button:last-child'),
    };
  });
  expect(boxes.footer.top).toBeGreaterThanOrEqual(boxes.paragraph.bottom);
  expect(boxes.sources.right).toBeLessThanOrEqual(boxes.copy.left);
  expect(boxes.copy.left).toBeLessThan(boxes.read.left);
  await expect(footer).not.toHaveCSS('animation-name', 'none');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(footer).toHaveCSS('animation-name', 'none');
  await page.screenshot({ path: test.info().outputPath('qa-footer.png') });
});

test('stop and pipeline changes cancel the native speech request by ID', async ({ page }) => {
  await page.goto('/e2e/fixtures/qa-footer.html');
  await page.getByRole('button', { name: 'deepseek', exact: true }).click();
  const read = page.getByRole('button', { name: 'DeepSeek 朗读', exact: true });
  const stop = page.getByRole('button', { name: '停止朗读', exact: true });
  const actions = () => page.evaluate(() => (window as any).qaFooterFixture.playback);
  await read.click();
  await expect(stop).toBeVisible();
  await expect
    .poll(async () => (await actions()).map(({ command }) => command))
    .toEqual(['deepseek_tts_start']);
  await page.getByRole('button', { name: 'traditional', exact: true }).click();
  await expect(stop).toHaveCount(0);
  await expect
    .poll(async () => (await actions()).map(({ command }) => command))
    .toEqual(['deepseek_tts_start', 'deepseek_tts_stop']);
  const first = await actions();
  expect(first[1].requestId).toBe(first[0].requestId);
  await page.evaluate(() => {
    (window as any).qaFooterFixture.finish();
    (window as any).qaFooterFixture.holdResume();
  });
  await page.getByRole('button', { name: 'deepseek', exact: true }).click();
  await read.click();
  await stop.click();
  await page.evaluate(() => (window as any).qaFooterFixture.releaseResume());
  await expect(read).toBeVisible();
  await expect
    .poll(async () => (await actions()).map(({ command }) => command))
    .toEqual(['deepseek_tts_start', 'deepseek_tts_stop', 'deepseek_tts_stop']);
  const all = await actions();
  expect(all[2].requestId).not.toBe(all[0].requestId);
});
