import { expect, test } from '@playwright/test';
test.use({ baseURL: 'http://127.0.0.1:5326' });
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
