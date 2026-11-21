import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';

test('usage separates overview, services and six purpose summaries', async ({ page }) => {
  await page.goto('/');
  const usage = page.getByRole('button', { name: '用量', exact: true });
  const settings = page.getByRole('button', { name: '设置', exact: true });
  await expect(usage).toHaveCSS(
    'font-size',
    await settings.evaluate((node) => getComputedStyle(node).fontSize),
  );
  await usage.click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  await expect(page.getByText('语音识别记录')).toBeVisible();
  await expect(page.getByText('TTS 用量', { exact: true })).toBeVisible();
  await expect(page.getByText('已报告输入 Token', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '每日记录' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '服务与用途' })).toHaveCount(0);
  await expect(page.locator('[data-ol-page="usage"]')).toHaveAttribute(
    'data-transition-phase',
    'settled',
  );
  await page.screenshot({ path: test.info().outputPath('usage-dashboard.png') });
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '服务', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: '服务与用途' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '每日记录' })).toHaveCount(0);
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '用途', exact: true })
    .click();
  const clear = page.getByRole('button', { name: '清除已记录 Token', exact: true });
  await expect(clear).toBeVisible();
  const period = page.getByRole('group', { name: '统计时段' });
  await expect(period).toBeVisible();
  const positions = await clear.evaluate((node) => {
    const button = node.getBoundingClientRect();
    const toolbar = node.closest('.ol-usage-toolbar')!.getBoundingClientRect();
    const segment = node
      .closest('.ol-usage-toolbar')!
      .querySelector('.ol-segmented')!
      .getBoundingClientRect();
    return {
      right: toolbar.right - button.right,
      center: Math.abs(button.top + button.bottom - (segment.top + segment.bottom)) / 2,
    };
  });
  expect(positions.right).toBeLessThan(1);
  expect(positions.center).toBeLessThan(1);
  await page.setViewportSize({ width: 960, height: 600 });
  await page.locator('.ol-usage-page').evaluate((node) => {
    node.style.zoom = '1.1';
  });
  const summaries = page.getByRole('region', { name: '已报告的模型用量' });
  await expect(summaries.getByRole('heading')).toHaveCount(6);
  await expect(summaries.getByRole('heading', { name: '听写后处理' })).toBeVisible();
  await expect(summaries.getByText('缓存读取', { exact: true })).toHaveCount(6);
  await expect(summaries.getByText('缓存写入', { exact: true })).toHaveCount(6);
  await expect(summaries.getByText('缓存命中率', { exact: true })).toHaveCount(6);
  await expect(page.getByRole('region', { name: '最近记录列表' })).toHaveCount(0);
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1300, height: 835 });
  await page.locator('.ol-usage-page').evaluate((node) => {
    node.style.zoom = '1';
  });

  const dimensions = await clear.evaluate((node) => ({
    scroll: node.scrollHeight,
    client: node.clientHeight,
  }));
  expect(dimensions.scroll).toBeLessThanOrEqual(dimensions.client + 1);
  await page.screenshot({ path: test.info().outputPath('usage-history.png') });
  await clear.click();
  const confirmation = page.getByRole('dialog', { name: '清除已记录 Token' });
  await confirmation.getByRole('button', { name: '清除已记录 Token' }).click();
  await expect(summaries.locator('dd[data-empty]')).toHaveCount(30);
  await page.getByRole('button', { name: '近 7 天' }).click();
  await expect(page.getByRole('button', { name: '近 7 天' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page
    .getByRole('navigation', { name: '用量页面' })
    .getByRole('button', { name: '概览', exact: true })
    .click();
  await expect(page.getByRole('heading', { name: '每日记录' })).toBeVisible();
  await expect
    .poll(() =>
      page.locator('.ol-usage-chart-dates').evaluate((node) => node.getBoundingClientRect().bottom),
    )
    .toBeLessThan(835);
  await page.screenshot({ path: test.info().outputPath('usage-dashboard-light.png') });
});


test('usage invalidations update foreground data and defer reads while hidden or unmounted', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/?os=mac`);
  await page.getByRole('button', { name: '用量', exact: true }).click();
  await expect(page.getByRole('heading', { name: '用量', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '刷新', exact: true })).toHaveCount(0);
  const count = () => page.evaluate(async () => {
    const path = '/src/data/usage.ts';
    const { serviceUsageQuery } = await import(path);
    return serviceUsageQuery.getSnapshot().data?.records.length;
  });
  await expect.poll(count).toBe(2);
  await page.evaluate(async () => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
    const path = '/src/preview/usage.ts';
    const { clearPreviewServiceUsage } = await import(path);
    clearPreviewServiceUsage();
    await new Promise((resolve) => setTimeout(resolve, 100));
  });
  expect(await count()).toBe(2);
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect.poll(count).toBe(0);
  await page.getByRole('button', { name: '概览', exact: true }).first().click();
  await expect(page.locator('[data-ol-page="usage"]')).toHaveCount(0);
  const unchanged = await page.evaluate(async () => {
    const eventPath = '/src/preview/events.ts';
    const queryPath = '/src/data/usage.ts';
    const { publishPreviewChange } = await import(eventPath);
    const { serviceUsageQuery } = await import(queryPath);
    const before = serviceUsageQuery.getSnapshot();
    publishPreviewChange('service-usage');
    await new Promise((resolve) => setTimeout(resolve, 100));
    return serviceUsageQuery.getSnapshot() === before;
  });
  expect(unchanged).toBe(true);
});


test('native service-usage events invalidate after subscription, defer background reads and detach', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/native-overview.html?usage&deferUsage`);
  const fixture = () => page.evaluate(() => (window as unknown as { usageFixture: { reads(): number } }).usageFixture.reads());
  const change = () => page.evaluate(() => (window as unknown as { usageFixture: { change(): Promise<void> } }).usageFixture.change());
  await expect.poll(fixture).toBe(1);
  await change();
  await page.evaluate(() => (window as unknown as { usageFixture: { finish(): void } }).usageFixture.finish());
  const value = page.getByTestId('usage-version');
  await expect(value).toHaveText('1');
  const reads = await fixture();
  await page.evaluate(() => Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' }));
  await change();
  await change();
  expect(await fixture()).toBe(reads);
  await expect(value).toHaveText('1');
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect(value).toHaveText('3');
  await page.getByRole('button', { name: 'Toggle usage subscriber' }).click();
  await expect(value).toHaveCount(0);
  const stopped = await fixture();
  await change();
  expect(await fixture()).toBe(stopped);
  await page.getByRole('button', { name: 'Toggle usage subscriber' }).click();
  await expect(value).toHaveText('4');
});


test('a delayed usage subscription can resume after backgrounding and releases after unmount', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/native-overview.html?usage&delayUsageSubscription`);
  const probe = () => page.evaluate(() => {
    const f = (window as unknown as { usageFixture: { reads(): number; listeners(): number } }).usageFixture;
    return { reads: f.reads(), listeners: f.listeners() };
  });
  await expect(page.getByTestId('usage-version')).toHaveText('loading');
  expect((await probe()).reads).toBe(0);
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    (window as unknown as { usageFixture: { finishListening(): void } }).usageFixture.finishListening();
  });
  await expect.poll(async () => (await probe()).listeners).toBe(1);
  expect((await probe()).reads).toBe(0);
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    window.dispatchEvent(new Event('focus'));
  });
  await expect(page.getByTestId('usage-version')).toHaveText('0');
  await page.getByRole('button', { name: 'Toggle usage subscriber' }).click();
  await expect.poll(async () => (await probe()).listeners).toBe(0);

  await page.reload();
  await expect(page.getByTestId('usage-version')).toHaveText('loading');
  await page.getByRole('button', { name: 'Toggle usage subscriber' }).click();
  await page.evaluate(() => (window as unknown as { usageFixture: { finishListening(): void } }).usageFixture.finishListening());
  await expect.poll(async () => (await probe()).listeners).toBe(0);
  expect((await probe()).reads).toBe(0);
});

test('usage subscription failure becomes query error and focus retries it', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/native-overview.html?usage&failUsageSubscription`);
  await expect(page.getByTestId('usage-version')).toHaveText('error');
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect(page.getByTestId('usage-version')).toHaveText('0');
});
