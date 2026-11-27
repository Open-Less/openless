import { expect, test } from '@playwright/test';
import { settleNavigation } from './support/navigation';
import { fixtureOrigin } from './support/server';

test.use({ baseURL: fixtureOrigin });

test('Web Search retains service secrets after switching, saving and reopening', async ({
  page,
}) => {
  const tavilyKey = 'search-fixture-key';
  await page.goto('/?os=mac');
  await page.evaluate(async (apiKey) => {
    const { setCredential } = await import('/src/api/providers.ts');
    await setCredential(
      'web_search.config',
      JSON.stringify({ provider: 'tavily', apiKey, endpoint: '' }),
    );
  }, tavilyKey);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await settleNavigation(page);
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  const openSearch = async () => {
    await dialog.getByRole('button', { name: '高级功能', exact: true }).click();
    await settleNavigation(page);
    await dialog.getByRole('button', { name: /Web Search/ }).click();
    await settleNavigation(page);
  };
  await openSearch();
  const select = async (name: string) => {
    await page.getByRole('combobox', { name: '搜索服务', exact: true }).click();
    await page.getByRole('option', { name, exact: true }).click();
  };
  await select('免费聚合搜索 + DeepSeek Web Search');
  await page.getByLabel('Search API Key').fill('deepseek-fixture');
  await select('SearXNG');
  await page.getByLabel('SearXNG URL', { exact: true }).fill('https://search.example/search');
  await select('免费聚合搜索');
  await dialog.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('status').filter({ hasText: '已保存' })).toBeVisible();
  await dialog.getByRole('button', { name: '返回应用', exact: true }).click();
  await settleNavigation(page);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await settleNavigation(page);
  await openSearch();
  await select('Tavily');
  await expect(page.getByLabel('Search API Key')).toHaveValue(tavilyKey);
  await select('免费聚合搜索 + DeepSeek Web Search');
  await expect(page.getByLabel('Search API Key')).toHaveValue('deepseek-fixture');
  await expect(dialog.getByText(/每千次非空成功请求/)).toBeVisible();
  await select('SearXNG');
  await expect(page.getByLabel('SearXNG URL', { exact: true })).toHaveValue(
    'https://search.example/search',
  );
  await page.setViewportSize({ width: 850, height: 750 });
  await select('免费聚合搜索 + DeepSeek Web Search');
  const trigger = page.getByRole('combobox', { name: '搜索服务', exact: true });
  expect(await trigger.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBeTruthy();
  await page.evaluate(async () => {
    await (await import('/src/i18n/index.ts')).default.changeLanguage('en');
  });
  const englishTrigger = page.getByRole('combobox', { name: 'Search provider', exact: true });
  await expect(englishTrigger).toContainText('Free Aggregated Search');
  await englishTrigger.click();
  await page
    .getByRole('option', { name: 'Free Aggregated Search + DeepSeek Web Search', exact: true })
    .click();
  expect(await englishTrigger.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBeTruthy();
});
