import { expect, test, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';

type Operation = 'credential' | 'rename' | 'delete';

interface ChannelFixture {
  calls: string[];
  saved(): { name: string; credential?: string };
  reject(operation?: Operation): void;
  hold(operation?: Operation): void;
  release(operation?: Operation): void;
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
  const name = editor.getByLabel('渠道名称（可选）', { exact: true });
  await expect(credential).toBeEnabled();
  return { editor, credential, provider, name };
}

test('Save waits for the latest name and credentials and keeps the editor open', async ({
  page,
}) => {
  const { editor, credential, name } = await openEditor(page);
  const save = editor.getByRole('button', { name: '保存', exact: true });
  const success = page.locator('.ol-notification-card[data-state="saved"]').filter({
    hasText: '渠道已保存',
  });
  await page.evaluate(() => {
    window.channelFixture.hold('rename');
    window.channelFixture.hold();
  });
  await name.fill('First name');
  await credential.fill('fixture credential');
  await expect
    .poll(() => page.evaluate(() => window.channelFixture.calls))
    .toContain('rename-start');
  await name.fill(' Latest name ');
  // Saving does not depend on whether clicking the button blurs the currently focused field.
  await save.evaluate((button: HTMLButtonElement) => button.click());
  await expect(save).toBeDisabled();
  await page.evaluate(() => window.channelFixture.release('rename'));
  await expect
    .poll(() => page.evaluate(() => window.channelFixture.saved().name))
    .toBe('Latest name');
  await expect(success).toHaveCount(0);
  await page.evaluate(() => window.channelFixture.release());
  await expect(success).toBeVisible();
  await expect(save).toBeEnabled();
  expect(await page.evaluate(() => window.channelFixture.saved())).toEqual({
    name: 'Latest name',
    credential: 'fixture credential',
  });
  await expect(editor).toBeVisible();
});

test('a failed rename survives Save and the upper Close waits for a successful retry', async ({
  page,
}) => {
  const { editor, name } = await openEditor(page);
  const save = editor.getByRole('button', { name: '保存', exact: true });
  const close = editor.getByRole('button', { name: '关闭', exact: true });
  await page.evaluate(() => window.channelFixture.reject('rename'));
  await name.fill('Retry name');
  await save.click();
  await expect(page.locator('.ol-notification-card[data-state="failed"]')).toContainText(
    '保存渠道失败，请重试。',
  );
  await expect(name).toHaveValue('Retry name');
  await expect(close).toHaveCount(1);
  await page.evaluate(() => window.channelFixture.hold('rename'));
  await close.click();
  await expect(save).toBeDisabled();
  await expect(editor).toBeVisible();
  await page.evaluate(() => window.channelFixture.release('rename'));
  await expect(page.getByText('Editor closed')).toBeVisible();
  expect(await page.evaluate(() => window.channelFixture.saved().name)).toBe('Retry name');
});

test('delete progress becomes a warning on rejection and success only after a confirmed retry', async ({
  page,
}) => {
  const { editor } = await openEditor(page);
  await editor.getByRole('button', { name: '删除渠道', exact: true }).click();
  const confirm = page.getByRole('dialog', { name: '删除渠道', exact: true });
  const accept = confirm.getByRole('button', { name: '确认删除', exact: true });
  const toast = page.locator('.ol-notification-card');
  await page.evaluate(() => window.channelFixture.hold('delete'));
  await accept.click();
  await expect(toast).toHaveAttribute('data-state', 'deleting');
  await expect(toast.locator('[data-slot="toast-icon"]')).toHaveAttribute('data-tone', 'trash');
  await expect(accept).toBeDisabled();
  await page.evaluate(() => window.channelFixture.reject('delete'));
  await expect(toast.getByRole('alert')).toHaveText('删除渠道失败，请重试。');
  await expect(toast).toHaveAttribute('data-severity', 'warning');
  await expect(toast.locator('[data-slot="toast-icon"]')).toHaveAttribute('data-tone', 'warning');
  await expect(toast).not.toHaveAttribute('data-state', 'deleted');
  await expect(accept).toBeEnabled();

  await page.evaluate(() => window.channelFixture.hold('delete'));
  await accept.click();
  await expect(toast).toHaveAttribute('data-state', 'deleting');
  await expect(page.getByText('Editor closed')).toHaveCount(0);
  await page.evaluate(() => window.channelFixture.release('delete'));
  await expect(page.getByText('Editor closed')).toBeVisible();
  await expect(toast).toHaveAttribute('data-state', 'deleted');
  await expect(toast.locator('[data-slot="toast-icon"]')).toHaveAttribute('data-tone', 'trash');
  await expect(toast).toContainText('已删除');
});

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
