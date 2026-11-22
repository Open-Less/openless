import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { settleNavigation, settleSelectDismissal } from './support/navigation';
import { fixtureOrigin } from './support/server';

const catalog = JSON.parse(
  readFileSync(
    new URL('../crates/openless-core/tests/fixtures/app-catalog.json', import.meta.url),
    'utf8',
  ),
);

test.beforeEach(async ({ page }) => {
  await page.route('https://apic.openless.top/v1/app-catalog', (route) =>
    route.fulfill({ json: catalog }),
  );
  await page.route('**/src/preview/appStyleRouting.ts*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
import { publishPreviewChange } from '/src/preview/events.ts';
routing.apps = [
  ['com.microsoft.VSCode', 'Visual Studio Code'], ['com.tencent.xinWeChat', 'WeChat'],
  ['com.apple.mail', 'Mail'], ['com.apple.Notes', 'Notes'], ['org.example.custom', 'Custom Editor'],
].map(([id, name]) => ({app: {id: 'macos:' + id, name}, memberId: 'macos:' + id, category: null, seen: true, stylePackId: null}));
window.addEventListener('fixture:dictation-completed', () => {
  routing.apps.push(
    {app: {id: 'macos:com.openai.codex', name: 'ChatGPT'}, memberId: 'catalog:chatgpt', category: null, seen: true, stylePackId: null},
    ...Array.from({length: 35}, (_, i) => ({app: {id: 'macos:org.example.' + i, name: 'Writing app ' + i}, memberId: 'macos:org.example.' + i, category: null, seen: true, stylePackId: null})),
  );
  publishPreviewChange('history');
});`,
    });
  });
});

async function openPage(page: Page) {
  await page.goto(`${fixtureOrigin}/?os=mac`);
  await page.getByRole('button', { name: '风格', exact: true }).click();
  await page.getByRole('button', { name: '按应用选择风格', exact: true }).click();
  await settleNavigation(page);
  await expect(page.getByRole('combobox', { name: '编程的风格包' })).toBeVisible();
  return page.locator('.ol-app-style-page');
}

async function choose(page: Page, target: string, name: string) {
  await page.getByRole('combobox', { name: `${target}的风格包`, exact: true }).click();
  await page.getByRole('option', { name, exact: true }).click();
  await settleSelectDismissal(page);
}

test('shares packs across categories and apps from the third Styles page', async ({ page }) => {
  const surface = await openPage(page);
  await expect(surface.locator('.ol-app-style-panel')).toHaveCount(2);
  await choose(page, '聊天', '清晰结构');
  await choose(page, '邮件', '清晰结构');
  await expect(page.getByRole('combobox', { name: 'WeChat的风格包', exact: true })).toHaveText(
    '跟随分类（清晰结构）',
  );
  await choose(page, 'WeChat', '清晰结构');
  await choose(page, 'Mail', '清晰结构');
  await choose(page, '邮件', '正式表达');
  await expect(page.getByRole('combobox', { name: '聊天的风格包', exact: true })).toHaveText(
    '清晰结构',
  );
  await expect(page.getByRole('combobox', { name: 'Mail的风格包', exact: true })).toHaveText(
    '清晰结构',
  );
  await surface.getByRole('button', { name: '清理应用记录' }).click();
  await page
    .getByRole('dialog', { name: '清理应用记录' })
    .getByRole('button', { name: '清理应用记录' })
    .click();
  await expect(
    page.getByRole('combobox', { name: 'Visual Studio Code的风格包', exact: true }),
  ).toHaveCount(0);
  await expect(page.getByRole('combobox', { name: 'WeChat的风格包', exact: true })).toHaveText(
    '清晰结构',
  );
});

test('preselects official apps, moves a conflicting app, and assigns a newly observed app', async ({
  page,
}) => {
  await openPage(page);
  await choose(page, '编程', '清晰结构');
  await choose(page, '聊天', '正式表达');
  await page.getByRole('button', { name: '编程的应用', exact: true }).click();
  const chatgpt = page.getByRole('checkbox', { name: 'ChatGPT', exact: true });
  await expect(chatgpt).toBeChecked();
  const wechat = page.getByRole('checkbox', { name: 'WeChat', exact: true });
  await expect(wechat).toHaveAttribute('data-conflict', 'true');
  await wechat.click();
  const move = page.getByRole('dialog', { name: '移动应用' });
  await expect(move).toContainText('聊天');
  await expect(move).toContainText('正式表达');
  await move.getByRole('button', { name: '移到此分类' }).click();
  await expect(page.getByRole('combobox', { name: 'WeChat的风格包', exact: true })).toHaveText(
    '跟随分类（清晰结构）',
  );
  await page.getByRole('button', { name: '编程的应用', exact: true }).click();
  await page.getByRole('checkbox', { name: 'Custom Editor', exact: true }).click();
  await expect(
    page.getByRole('combobox', { name: 'Custom Editor的风格包', exact: true }),
  ).toHaveText('跟随分类（清晰结构）');
  await page.getByRole('checkbox', { name: 'Visual Studio Code', exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(
    page.getByRole('combobox', { name: 'Visual Studio Code的风格包', exact: true }),
  ).toHaveText('使用当前手选风格');
});

test('refreshes inherited styles after dictation and scrolls each large panel independently', async ({
  page,
}) => {
  await page.setViewportSize({ width: 980, height: 640 });
  const surface = await openPage(page);
  await choose(page, '编程', '清晰结构');
  const clear = surface.getByRole('button', { name: '清理应用记录' });
  const before = await clear.boundingBox();
  await page.evaluate(() => window.dispatchEvent(new Event('fixture:dictation-completed')));
  await expect(page.getByRole('combobox', { name: 'ChatGPT的风格包', exact: true })).toHaveText(
    '跟随分类（清晰结构）',
  );
  const body = surface.locator('.ol-app-style-panel-body').nth(1);
  expect(await body.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  await page
    .getByRole('combobox', { name: 'Writing app 9的风格包', exact: true })
    .scrollIntoViewIfNeeded();
  const after = await clear.boundingBox();
  expect(after!.y).toBeCloseTo(before!.y, 0);
  expect(await surface.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
});

test('keeps the new page usable with longer translated labels and larger text', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol.locale', 'de'));
  await page.setViewportSize({ width: 980, height: 640 });
  await page.goto(`${fixtureOrigin}/?os=win`);
  await page.locator('html').evaluate((element) => {
    element.style.fontSize = '125%';
  });
  await page.getByRole('button', { name: 'Stil', exact: true }).click();
  await page.getByRole('button', { name: 'Stile nach App', exact: true }).click();
  await settleNavigation(page);
  const picker = page.getByRole('combobox', { name: 'Stilpaket für Notes', exact: true });
  await expect(picker).toBeVisible();
  await picker.click();
  await expect(
    page.getByRole('option', { name: 'Manuell gewählten Stil verwenden', exact: true }),
  ).toBeVisible();
  await page.keyboard.press('Escape');
  expect(
    await page
      .locator('.ol-app-style-page')
      .evaluate((node) => node.scrollWidth <= node.clientWidth),
  ).toBe(true);
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
  const editor = page.getByRole('dialog', { name: '编辑风格', exact: true });
  const name = editor.getByRole('textbox', { name: '名称', exact: true });
  await expect(name).toHaveValue('未命名风格');
  await name.fill('新建的独立风格');
  await editor.getByRole('button', { name: '保存', exact: true }).click();
  await expect(name).toHaveValue('新建的独立风格');
  await expect(editor.getByRole('button', { name: '激活', exact: true })).toBeEnabled();
});
