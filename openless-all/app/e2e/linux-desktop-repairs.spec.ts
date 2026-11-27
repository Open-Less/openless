import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';
const fixture = `${fixtureOrigin}/e2e/fixtures/linux-desktop-repairs.html`;

test('conflict review identifies each local/cloud style without exposing the document', async ({ page }) => {
  await page.goto(`${fixture}?mode=review`);
  await expect(page.getByText('本地写作风格', { exact: true }).first()).toBeVisible();
  await expect(page.getByText('云端写作风格', { exact: false }).first()).toBeVisible();
  await expect(page.getByText('翻译风格', { exact: true }).first()).toBeVisible();
  const fields = page.locator('fieldset').filter({ has: page.getByRole('radio') });
  await expect(fields).toHaveCount(3);
});

test('ordinary warning expires without user dismissal', async ({ page }) => {
  await page.clock.install();
  await page.goto(`${fixture}?mode=notices`);
  await page.getByRole('button', { name: 'Warn' }).click();
  await expect(page.getByText('会自动关闭的警告')).toBeVisible();
  await page.clock.fastForward(8500);
  await expect(page.getByText('会自动关闭的警告')).toHaveCount(0);
});

test('Linux QA view submits to Core and renders its returned answer', async ({ page }) => {
  await page.goto(fixture);
  await page.getByRole('textbox', { name: '问题', exact: true }).fill('测试问题');
  await page.getByRole('button', { name: '发送', exact: true }).click();
  await expect(page.getByText('测试回答 / test answer')).toBeVisible();
  const calls = await page.evaluate(() => (window as any).fixture.calls);
  expect(calls.some((call: any) => call.command === 'qa_submit_text' && call.args.text === '测试问题')).toBe(true);
  await page.getByRole('button', { name: '关闭', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).fixture.calls.some((call: any) => call.command === 'qa_window_dismiss'))).toBe(true);
});

test('correction add button sizes to its label instead of stretching a dark grid cell', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto(`${fixture}?mode=corrections`);
  const button = page.locator('.ol-correction-add');
  await expect(button).toBeVisible();
  expect((await button.boundingBox())!.width).toBeLessThan(100);
  await expect(button).toHaveCSS('justify-content', 'center');
});
