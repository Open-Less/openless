import { expect, test } from '@playwright/test';
import { settleNavigation } from './support/navigation';
test.use({ baseURL: 'http://127.0.0.1:5326' });

test('three pipelines retain separate credentials and the selected pipeline after settings close', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  const baseline = await page.evaluate(async () => {
    const ipc = await import('/src/lib/ipc/index.ts');
    const id = await ipc.createChannel('llm', 'deepseek', 'Existing DeepSeek');
    await ipc.setCredential('ark.api_key', 'traditional-fixture-key', id);
    await ipc.setCredential('ark.endpoint', 'https://api.deepseek.com/v1', id);
    await ipc.setCredential('omni.api_key', 'omni-fixture-key', 'custom');
    return { id, asr: await ipc.listChannels('asr'), llm: await ipc.listChannels('llm') };
  });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await settleNavigation(page);
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
  const pipeline = dialog.getByRole('group', { name: '识别管线', exact: true });
  await expect(pipeline.getByRole('button')).toHaveCount(3);
  await expect(pipeline.getByRole('button').nth(2)).toHaveText('DeepSeek 大统一');
  await pipeline.getByRole('button', { name: 'DeepSeek 大统一', exact: true }).click();
  await settleNavigation(page);
  await expect(page.getByLabel('DeepSeek suite API Key')).toHaveValue('traditional-fixture-key');
  await expect(dialog.locator('.ol-service-views')).toHaveCount(0);
  await page.getByLabel('DeepSeek suite API Key').fill('dedicated-fixture-key');
  await page.getByLabel('朗读音色', { exact: true }).selectOption('tide');
  await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
  await expect(dialog.getByRole('status')).toContainText('已保存独立配置');
  for (const mode of ['传统模式', '多模态模式', 'DeepSeek 大统一']) {
    await pipeline.getByRole('button', { name: mode, exact: true }).click();
    await expect(pipeline.getByRole('button', { name: mode, exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await dialog.getByRole('button', { name: '返回应用', exact: true }).click();
    await settleNavigation(page);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await settleNavigation(page);
    await dialog.getByRole('button', { name: 'AI 与模型', exact: true }).click();
    await settleNavigation(page);
    await expect(pipeline.getByRole('button', { name: mode, exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  }
  await expect(page.getByLabel('DeepSeek suite API Key')).toHaveValue('dedicated-fixture-key');
  const after = await page.evaluate(async (id) => {
    const ipc = await import('/src/lib/ipc/index.ts');
    return {
      asr: await ipc.listChannels('asr'),
      llm: await ipc.listChannels('llm'),
      key: await ipc.readCredential('ark.api_key', id),
      omni: await ipc.readCredential('omni.api_key', 'custom'),
      suite: JSON.parse((await ipc.readCredential('deepseek_suite.config'))!),
    };
  }, baseline.id);
  expect(after.asr).toEqual(baseline.asr);
  expect(after.llm).toEqual(baseline.llm);
  expect(after.key).toBe('traditional-fixture-key');
  expect(after.omni).toBe('omni-fixture-key');
  expect(after.suite).toEqual({ apiKey: 'dedicated-fixture-key', voice: 'tide' });
  await page.screenshot({ path: test.info().outputPath('three-pipelines.png') });
});
