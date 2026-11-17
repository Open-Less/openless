import { expect, test, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';

interface ChannelFixture {
  calls: string[];
  reject(): void;
  hold(): void;
  release(): void;
  forceClose(): void;
}

declare global {
  interface Window {
    channelFixture: ChannelFixture;
  }
}

async function openEditor(page: Page) {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/channel-draft.html`);
  const editor = page.getByRole('dialog', { name: '编辑渠道' });
  const credential = editor.getByLabel('API 密钥', { exact: true });
  const provider = editor.getByRole('combobox', { name: '供应商' });
  await expect(credential).toBeEnabled();
  return { editor, credential, provider };
}

test('provider changes only after the old credential is saved; a failed write keeps the draft', async ({
  page,
}) => {
  const { editor, credential, provider } = await openEditor(page);
  await page.evaluate(() => window.channelFixture.reject());
  await credential.fill('fixture credential');
  await provider.click();
  await page.getByRole('option', { name: 'DeepSeek', exact: true }).click();
  await expect(editor.getByRole('alert')).toContainText('操作失败');
  await expect(credential).toHaveValue('fixture credential');
  await expect(provider).not.toContainText('DeepSeek');
  const calls = () => page.evaluate(() => window.channelFixture.calls as string[]);
  expect(await calls()).not.toContain('provider:deepseek');

  await page.evaluate(() => window.channelFixture.release());
  await provider.click();
  await page.getByRole('option', { name: 'DeepSeek', exact: true }).click();
  await expect(provider).toContainText('DeepSeek');
  const saved = await calls();
  expect(saved.filter((call) => call === 'provider:deepseek')).toHaveLength(1);
  expect(saved.indexOf('credential-committed')).toBeLessThan(saved.indexOf('provider:deepseek'));
});

test('invalid protocol settings block a provider switch without discarding the draft', async ({
  page,
}) => {
  const { editor, provider } = await openEditor(page);
  const limit = editor.getByRole('spinbutton', { name: '最大输出 tokens' });
  await expect(limit).toBeEnabled();
  await limit.fill('0');
  await provider.click();
  await page.getByRole('option', { name: 'DeepSeek', exact: true }).click();
  await expect(editor.getByRole('alert').filter({ hasText: '操作失败' })).toBeVisible();
  await expect(limit).toHaveValue('0');
  await expect(provider).not.toContainText('DeepSeek');
  expect(await page.evaluate(() => window.channelFixture.calls)).not.toContain('provider:deepseek');
});

for (const exit of ['close', 'unmount'] as const) {
  test(`a queued provider switch cannot commit after editor ${exit}`, async ({ page }) => {
    const { editor, credential, provider } = await openEditor(page);
    await page.evaluate(() => window.channelFixture.hold());
    await credential.fill('held fixture credential');
    await provider.click();
    await page.getByRole('option', { name: 'DeepSeek', exact: true }).click();
    await expect(provider).toBeDisabled();
    if (exit === 'close') {
      await editor.getByRole('button', { name: '关闭' }).first().click();
    } else {
      await page.evaluate(() => window.channelFixture.forceClose());
      await expect(page.getByText('Editor closed')).toBeVisible();
    }
    await page.evaluate(() => window.channelFixture.release());
    await expect(page.getByText('Editor closed')).toBeVisible();
    await expect
      .poll(() => page.evaluate(() => window.channelFixture.calls))
      .toContain('credential-committed');
    expect(await page.evaluate(() => window.channelFixture.calls)).not.toContain(
      'provider:deepseek',
    );
  });
}
