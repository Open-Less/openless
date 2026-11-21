import { expect, test } from '@playwright/test';

test('dictionary deletion waits for confirmation and keeps selection after cancellation', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: '词典', exact: true }).click();
  const dictionary = page.locator('[data-ol-page="vocab"]');
  await expect(dictionary).toHaveAttribute('data-transition-phase', 'settled');
  await dictionary.getByRole('button', { name: '选择', exact: true }).click();
  await dictionary.getByRole('checkbox', { name: '选择当前结果' }).check();
  const remove = dictionary.getByRole('button', { name: /^删除已选/ });
  await remove.click();
  const confirmation = page.getByRole('dialog', { name: /^删除已选/ });
  await confirmation.getByRole('button', { name: '取消', exact: true }).click();
  await expect(remove).toBeFocused();
  await expect(remove).toBeVisible();
  await remove.click();
  await confirmation.getByRole('button', { name: '删除', exact: true }).click();
  await expect(remove).toHaveCount(0);
  await expect(dictionary.getByRole('checkbox', { name: '选择当前结果' })).toBeDisabled();
});

test('style editor retains its draft when discard is cancelled', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  const style = page.locator('[data-ol-page="style"]');
  await expect(style).toHaveAttribute('data-transition-phase', 'settled');
  await style
    .getByRole('button', { name: '编辑', exact: true })
    .and(style.locator('button:enabled'))
    .first()
    .click();
  const editor = style.getByRole('dialog', { name: /编辑/ });
  const prompt = editor.locator('textarea:visible').first();
  await expect(prompt).toBeVisible();
  await prompt.fill('A draft that should survive a cancelled discard.');
  await editor.getByRole('button', { name: '关闭', exact: true }).click();
  const discard = page.getByRole('dialog', { name: '放弃修改？' });
  await discard.getByRole('button', { name: '取消', exact: true }).click();
  await expect(prompt).toHaveValue('A draft that should survive a cancelled discard.');
  await editor.getByRole('button', { name: '关闭', exact: true }).click();
  await discard.getByRole('button', { name: '确认', exact: true }).click();
  await expect(editor).not.toBeVisible();
});
