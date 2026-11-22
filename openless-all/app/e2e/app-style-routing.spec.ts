import { expect, test } from '@playwright/test';
import { settleNavigation, settleSelectDismissal } from './support/navigation';
import { fixtureOrigin } from './support/server';

test('shares style packs across apps and categories with one selection per target', async ({
  page,
}) => {
  await page.addInitScript(() => {
    document.addEventListener(
      'keydown',
      (event) => {
        if (event.key === 'Escape') {
          // Exercise a popup that drops its open marker during document capture.
          document.querySelector('.ol-select-positioner[data-open]')?.removeAttribute('data-open');
        }
      },
      { capture: true, once: true },
    );
  });
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await settleNavigation(page);
  const open = page.getByRole('button', { name: '按应用选择风格' });
  await open.click();

  const dialog = page.getByRole('dialog', { name: '按应用选择风格' });
  const packFor = (name: string) => dialog.getByRole('combobox', { name: `${name}的风格包` });
  const popup = page.locator('.ol-select-popup[data-open]');
  const chat = packFor('聊天');
  const email = packFor('邮件');
  const wechat = packFor('WeChat');
  const mail = packFor('Mail');
  const choose = async (target: typeof chat, name: string) => {
    await target.click();
    await expect(popup).toHaveCSS('opacity', '1');
    await popup.getByRole('option', { name, exact: true }).click();
    await settleSelectDismissal(page);
  };

  await choose(chat, '清晰结构');
  await email.click();
  await expect(email).toHaveAttribute('aria-expanded', 'true');
  await expect(popup).toHaveCSS('opacity', '1');
  await expect(popup.getByRole('option', { name: '清晰结构', exact: true })).toBeEnabled();
  await page.keyboard.press('Escape');
  await settleSelectDismissal(page);
  await expect(dialog).toBeVisible();

  await choose(email, '清晰结构');
  await expect(chat).toHaveText('清晰结构');
  await expect(email).toHaveText('清晰结构');
  await expect(wechat).toHaveText('跟随分类（清晰结构）');
  await expect(mail).toHaveText('跟随分类（清晰结构）');

  await choose(wechat, '清晰结构');
  await choose(mail, '清晰结构');
  await choose(email, '正式表达');
  await expect(email).toHaveText('正式表达');
  await expect(chat).toHaveText('清晰结构');
  await expect(mail).toHaveText('清晰结构');
  await expect(wechat).toHaveText('清晰结构');

  await dialog.getByRole('button', { name: '清理应用记录' }).click();
  await page
    .getByRole('dialog', { name: '清理应用记录' })
    .getByRole('button', { name: '清理应用记录' })
    .click();
  await expect(packFor('Visual Studio Code')).toHaveCount(0);
  await expect(wechat).toHaveText('清晰结构');
  await expect(mail).toHaveText('清晰结构');

  await choose(wechat, '跟随分类（清晰结构）');
  await expect(wechat).toHaveCount(0);
  await expect(mail).toHaveText('清晰结构');
  await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(open).toBeFocused();
  await open.click();
  await expect(chat).toHaveText('清晰结构');
  await expect(email).toHaveText('正式表达');
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
  await settleNavigation(page);
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

test('refreshes newly dictated apps with inherited styles inside the dialog scroll area', async ({
  page,
}) => {
  await page.route('**/src/preview/appStyleRouting.ts*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
import { publishPreviewChange } from '/src/preview/events.ts';
window.addEventListener('fixture:dictation-completed', () => {
  routing.apps.push(
    { app: { id: 'macos:com.openai.codex', name: 'ChatGPT' }, category: 'coding', seen: true, stylePackId: null },
    { app: { id: 'macos:com.stablyai.orca', name: 'Orca' }, category: 'coding', seen: true, stylePackId: null },
    ...Array.from({ length: 35 }, (_, index) => ({
      app: { id: 'macos:org.example.' + index, name: 'Writing app ' + index }, category: null, seen: true, stylePackId: null,
    })),
  );
  publishPreviewChange('history');
});`,
    });
  });
  await page.setViewportSize({ width: 980, height: 640 });
  await page.goto(`${fixtureOrigin}/?os=mac`);
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await settleNavigation(page);
  await page.getByRole('button', { name: '按应用选择风格' }).click();
  const dialog = page.getByRole('dialog', { name: '按应用选择风格' });
  await dialog.getByRole('combobox', { name: '编程的风格包' }).click();
  await page.getByRole('option', { name: '清晰结构', exact: true }).click();
  await settleSelectDismissal(page);
  const footer = await dialog.getByRole('button', { name: '清理应用记录' }).boundingBox();
  await page.evaluate(() => window.dispatchEvent(new Event('fixture:dictation-completed')));
  for (const app of ['ChatGPT', 'Orca']) {
    await expect(dialog.getByRole('combobox', { name: `${app}的风格包` })).toHaveText(
      '跟随分类（清晰结构）',
    );
  }
  await expect(dialog.getByRole('heading', { name: '指定应用' })).toBeVisible();
  const body = dialog.locator('.ol-dialog-body');
  expect(await body.evaluate((element) => element.scrollHeight > element.clientHeight)).toBe(true);
  const lastApp = dialog.getByRole('combobox', { name: 'Writing app 9的风格包' });
  await lastApp.scrollIntoViewIfNeeded();
  await lastApp.click();
  await expect(page.getByRole('option', { name: '使用当前手选风格', exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  const after = await dialog.getByRole('button', { name: '清理应用记录' }).boundingBox();
  expect(after!.y).toBeCloseTo(footer!.y, 0);
  expect(await body.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true);
});

test('opens and saves the newly created pack despite overlapping change events', async ({
  page,
}) => {
  await page.route('**/src/lib/ipc/style-packs.ts*', async (route) => {
    const response = await route.fetch();
    const source = (await response.text())
      .replace(
        '() => cloneMockStylePacks());',
        '() => cloneMockStylePacks()).then(value => new Promise(resolve => setTimeout(() => resolve(value), 80)));',
      )
      .replace(
        'mockCreateStylePackFromTemplate(template)',
        `(() => {
        const created = mockCreateStylePackFromTemplate(template);
        setTimeout(() => publishPreviewChange('style-packs'), 10);
        return created;
      })()`,
      );
    await route.fulfill({
      response,
      body: source + "\nimport { publishPreviewChange } from '/src/preview/events.ts';",
    });
  });
  await page.goto(`${fixtureOrigin}/?os=mac`);
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '润色模式', exact: true }).click();
  await settleNavigation(page);
  await page.getByRole('button', { name: /新建风格/ }).click();
  const name = page.getByRole('textbox', { name: '名称', exact: true });
  await expect(name).toHaveValue('未命名风格');
  await name.fill('新建的独立风格');
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(name).toHaveValue('新建的独立风格');
  await expect(
    page
      .getByRole('dialog', { name: '编辑风格', exact: true })
      .getByRole('button', { name: '激活', exact: true }),
  ).toBeEnabled();
});
