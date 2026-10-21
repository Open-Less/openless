import { test, expect } from '@playwright/test';
const fixture = 'http://127.0.0.1:5326/e2e/fixtures/siwc.html';
test('SIWC sign-in, cancellation, model selection, quota and disconnect', async ({ page }) => {
  await page.goto(fixture);
  await expect(page.getByText('尚未连接 ChatGPT', { exact: true })).toBeVisible();
  await expect(page.getByText('API Key', { exact: true })).toHaveCount(0);
  const primaryBackground = await page
    .getByRole('button', { name: 'Continue with ChatGPT', exact: true })
    .evaluate((el) => getComputedStyle(el).backgroundColor);
  await page.getByRole('button', { name: 'Continue with ChatGPT', exact: true }).click();
  await expect(page.getByText('请在系统浏览器中完成授权', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '取消登录', exact: true }).click();
  await expect(page.getByText('尚未连接 ChatGPT', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Continue with ChatGPT', exact: true }).click();
  await expect(page.getByText('请在系统浏览器中完成授权', { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).siwcFixture.complete());
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.getByRole('button', { name: '知道了', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByRole('button', { name: '测试连接', exact: true })).toBeDisabled();
  await page.getByRole('combobox', { name: '润色模型', exact: true }).click();
  await page.getByRole('option', { name: '账号可用模型', exact: true }).click();
  await expect(page.getByRole('button', { name: '测试连接', exact: true })).toBeEnabled();
  const inferenceBefore = await page.evaluate(
    () =>
      (window as any).siwcFixture.calls.filter(
        (c: any) => c.command === 'validate_provider_credentials',
      ).length,
  );
  expect(inferenceBefore).toBe(0);
  await expect(page.locator('[role="listbox"]')).toHaveCount(0);
  await page.screenshot({ path: 'test-results/siwc-connected.png', fullPage: true });
  await page.getByRole('button', { name: '测试连接', exact: true }).click();
  await expect(page.getByText('连接测试成功', { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).siwcFixture.quota());
  await page.getByRole('button', { name: '测试连接', exact: true }).click();
  await expect(page.getByText('已达到计划或此应用的用量限制', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '管理用量', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '管理用量', exact: true })).toHaveCSS(
    'background-color',
    primaryBackground,
  );
  await page.getByRole('button', { name: '断开连接', exact: true }).click();
  await expect(page.getByText(/已在本机断开连接，但未确认远端撤销/)).toBeVisible();
  const calls = await page.evaluate(() => (window as any).siwcFixture.calls);
  expect(calls.some((c: any) => /set_active|reorder|set_channel_enabled/.test(c.command))).toBe(
    false,
  );
});
test('SIWC account form fits a narrow screen and keyboard navigation', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(fixture);
  await page.getByRole('button', { name: 'Continue with ChatGPT', exact: true }).focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: '取消登录', exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).siwcFixture.complete());
  await page.getByRole('button', { name: '知道了', exact: true }).click();
  await expect(page.getByRole('combobox', { name: '润色模型', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.screenshot({ path: 'test-results/siwc-mobile.png', fullPage: true });
});

test('SIWC account selection and quota retry refresh after native null results', async ({
  page,
}) => {
  await page.goto(fixture);
  await page.getByRole('button', { name: 'Continue with ChatGPT', exact: true }).waitFor();
  await page.evaluate(() => (window as any).siwcFixture.complete());
  await page.getByRole('button', { name: '知道了', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('combobox', { name: '润色模型', exact: true }).click();
  await page.getByRole('option', { name: '账号可用模型', exact: true }).click();
  const accountPicker = page.getByRole('combobox', { name: 'ChatGPT 账号', exact: true });
  await accountPicker.click();
  await page.getByRole('option', { name: 'other@example.test · 000002', exact: true }).click();
  await expect(accountPicker).toContainText('other@example.test');
  const testConnection = page.getByRole('button', { name: '测试连接', exact: true });
  await expect(testConnection).toBeEnabled();
  await page.evaluate(() => (window as any).siwcFixture.quota());
  await testConnection.click();
  await expect(page.getByText('已达到计划或此应用的用量限制', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '重试计划用量', exact: true }).click();
  await expect(testConnection).toBeEnabled();
  await expect(page.getByText('已达到计划或此应用的用量限制', { exact: true })).toHaveCount(0);
});
