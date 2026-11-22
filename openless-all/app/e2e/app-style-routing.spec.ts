import { expect, test } from '@playwright/test';

test('assigns app and category styles independently and clears only app history', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  const open = page.getByRole('button', { name: '按应用选择风格' });
  await open.click();

  const dialog = page.getByRole('dialog', { name: '按应用选择风格' });
  const packFor = (name: string) => dialog.getByRole('combobox', { name: `${name}的风格包` });
  const coding = packFor('编程');
  await coding.click();
  await page.getByRole('option', { name: '清晰结构', exact: true }).click();
  await expect(coding).toHaveText('清晰结构');

  const chat = packFor('聊天');
  await chat.click();
  await expect(page.getByRole('option', { name: '清晰结构 · 已分配给 编程' })).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(dialog).toBeVisible();

  const editor = packFor('Visual Studio Code');
  await expect(editor).toHaveText('跟随分类（清晰结构）');
  await editor.click();
  await page.getByRole('option', { name: '清晰结构', exact: true }).click();
  await expect(editor).toHaveText('清晰结构');
  await packFor('WeChat').click();
  await expect(
    page.getByRole('option', { name: '清晰结构 · 已分配给 Visual Studio Code' }),
  ).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(dialog.getByText('WeChat', { exact: true })).toBeVisible();

  await dialog.getByRole('button', { name: '清理应用记录' }).click();
  await page
    .getByRole('dialog', { name: '清理应用记录' })
    .getByRole('button', { name: '清理应用记录' })
    .click();
  await expect(packFor('WeChat')).toHaveCount(0);
  await expect(editor).toHaveText('清晰结构');
  await expect(coding).toHaveText('清晰结构');

  await editor.click();
  await page.getByRole('option', { name: '跟随分类（清晰结构）', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(open).toBeFocused();
  await open.click();
  await expect(coding).toHaveText('清晰结构');
});

test('keeps app rules reachable at a narrow window size and larger text', async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 470 });
  await page.addInitScript(() => localStorage.setItem('ol.locale', 'de'));
  await page.goto('/?os=win');
  await page.locator('html').evaluate((element) => {
    element.style.fontSize = '125%';
  });

  // The route can be opened directly in the existing preview shell.
  await page.getByRole('button', { name: 'Stil', exact: true }).click();
  await page
    .getByRole('button', { name: /Überarbeitungsmodus|Glättungsmodus|Überarbeitungsstile/ })
    .click();
  await page.getByRole('button', { name: 'Stile nach App' }).click();

  const dialog = page.getByRole('dialog', { name: 'Stile nach App' });
  const notes = dialog.getByRole('combobox', { name: 'Stilpaket für Notes' });
  await notes.scrollIntoViewIfNeeded();
  const frame = await dialog.boundingBox();
  const field = await notes.boundingBox();
  expect(frame).not.toBeNull();
  expect(field).not.toBeNull();
  expect(field!.x).toBeGreaterThanOrEqual(frame!.x);
  expect(field!.x + field!.width).toBeLessThanOrEqual(frame!.x + frame!.width + 1);
  await notes.click();
  await expect(
    page.getByRole('option', { name: 'Manuell gewählten Stil verwenden' }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(dialog.getByRole('button', { name: 'App-Verlauf löschen' })).toBeVisible();
});
