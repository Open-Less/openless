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
  await page.getByRole('button', { name: '自动风格', exact: true }).click();
  await settleNavigation(page);
  await expect(page.getByRole('combobox', { name: '编程的风格包' })).toBeVisible();
  return page.locator('.ol-app-style-page');
}

async function choose(page: Page, target: string, name: string) {
  await (await revealField(page, `${target}的风格包`)).click();
  await page.getByRole('option', { name, exact: true }).click();
  await settleSelectDismissal(page);
}

async function revealField(page: Page, label: string) {
  const field = page.getByRole('combobox', { name: label, exact: true });
  await field.scrollIntoViewIfNeeded();
  return field;
}

test('shares packs across categories and apps from the third Styles page', async ({ page }) => {
  const surface = await openPage(page);
  await expect(surface.getByRole('heading', { name: '自动风格', exact: true })).toBeVisible();
  await expect(surface.getByRole('button', { name: '更新分类目录', exact: true })).toHaveCount(0);
  await expect(surface.locator('.ol-app-style-panel')).toHaveCount(2);
  await choose(page, '聊天', '清晰结构');
  await choose(page, '邮件', '清晰结构');
  await expect(await revealField(page, 'WeChat的风格包')).toHaveText('跟随分类（清晰结构）');
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
  const appDialog = page.getByRole('dialog', { name: '选择应用', exact: true });
  await expect(appDialog.getByRole('checkbox')).toHaveText([
    'ChatGPT',
    'Orca',
    'Visual Studio Code',
    'Custom Editor',
    'Mail',
    'Notes',
    'WeChat',
  ]);
  const wechat = page.getByRole('checkbox', { name: 'WeChat', exact: true });
  await expect(wechat).toHaveAttribute('data-conflict', 'true');
  await wechat.click();
  const move = page.getByRole('dialog', { name: '移动应用' });
  await expect(move.locator('.ol-dialog-footer')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(move.locator('.ol-dialog-footer')).toHaveCSS('border-top-width', '0px');
  await expect(move).toContainText('聊天');
  await expect(move).toContainText('正式表达');
  await move.getByRole('button', { name: '移到此分类' }).click();
  await expect(appDialog.locator('.ol-dialog-footer')).toHaveCSS(
    'background-color',
    'rgba(0, 0, 0, 0)',
  );
  await expect(appDialog.locator('.ol-dialog-footer')).toHaveCSS('border-top-width', '0px');
  await expect(appDialog.getByRole('checkbox', { name: 'WeChat', exact: true })).toBeChecked();
  await page.getByRole('checkbox', { name: 'Custom Editor', exact: true }).click();
  await expect(appDialog.getByRole('checkbox')).toHaveText([
    'ChatGPT',
    'Custom Editor',
    'Orca',
    'Visual Studio Code',
    'WeChat',
    'Mail',
    'Notes',
  ]);
  await page.getByRole('checkbox', { name: 'Visual Studio Code', exact: true }).click();
  await appDialog.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(await revealField(page, 'WeChat的风格包')).toHaveText('跟随分类（清晰结构）');
  await expect(await revealField(page, 'Custom Editor的风格包')).toHaveText('跟随分类（清晰结构）');
  await expect(await revealField(page, 'Visual Studio Code的风格包')).toHaveText(
    '使用当前手选风格',
  );
});

test('searches and scrolls compact app cards and category choices on the fixed board', async ({
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
  const right = surface.locator('.ol-app-style-panel').nth(1);
  await expect(right.getByRole('combobox')).toHaveCount(41);
  expect(await body.evaluate((node) => node.scrollHeight > node.clientHeight)).toBe(true);
  await body.hover();
  await page.mouse.wheel(0, 500);
  await expect.poll(() => body.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  const last = right.getByRole('combobox', { name: 'Writing app 34的风格包', exact: true });
  await last.scrollIntoViewIfNeeded();
  await expect(last).toBeInViewport();
  const search = right.getByRole('textbox', { name: '搜索应用' });
  await search.fill('  nOtEs  ');
  await expect(right.getByRole('combobox')).toHaveCount(1);
  await choose(page, 'Notes', '正式表达');
  const row = right.locator('.ol-app-style-row');
  const name = await row.locator('.ol-app-style-name').boundingBox();
  const field = await row.getByRole('combobox').boundingBox();
  expect(field!.x).toBeGreaterThanOrEqual(name!.x + name!.width);
  expect(field!.y + field!.height / 2).toBeCloseTo(name!.y + name!.height / 2, 0);
  await search.fill('no matching application');
  await expect(right.getByRole('combobox')).toHaveCount(0);
  await expect(right).toContainText('没有匹配的应用');
  await search.fill('');
  await expect(right.getByRole('combobox', { name: 'Notes的风格包', exact: true })).toHaveText(
    '正式表达',
  );
  const firstRows = right.locator('.ol-app-style-row');
  await firstRows.first().scrollIntoViewIfNeeded();
  const firstRow = await firstRows.nth(0).boundingBox();
  const nextRow = await firstRows.nth(1).boundingBox();
  expect(nextRow!.y - firstRow!.y - firstRow!.height).toBeLessThanOrEqual(12);
  await expect(right.getByRole('button', { name: '下一页', exact: true })).toHaveCount(0);
  const after = await clear.boundingBox();
  expect(after!.y).toBeCloseTo(before!.y, 0);
  expect(await surface.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
  expect(await surface.evaluate((node) => node.scrollHeight <= node.clientHeight + 1)).toBe(true);
  await expect(
    page.locator('[data-ol-page="appStyles"][data-transition-phase="settled"]'),
  ).toHaveCSS('overflow', 'hidden');
  await page.getByRole('button', { name: '编程的应用', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '选择应用', exact: true });
  await expect(dialog.locator('.ol-app-members-heading')).toContainText('编程');
  await dialog.getByRole('textbox', { name: '搜索应用' }).fill('Writing app');
  await expect(dialog.getByRole('checkbox')).toHaveCount(35);
  expect(
    await dialog
      .locator('.ol-dialog-body')
      .evaluate((node) => node.scrollHeight > node.clientHeight),
  ).toBe(true);
  const dialogBody = dialog.locator('.ol-dialog-body');
  await dialogBody.hover();
  await page.mouse.wheel(0, 500);
  await expect.poll(() => dialogBody.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  const lastApp = dialog.getByRole('checkbox', { name: 'Writing app 34', exact: true });
  await lastApp.scrollIntoViewIfNeeded();
  await expect(lastApp).toBeInViewport();
  await lastApp.click();
  await expect(dialog.getByRole('checkbox').first()).toHaveAttribute(
    'aria-label',
    'Writing app 34',
  );
  await expect(dialog.getByRole('checkbox').first()).toBeChecked();
  await expect(dialog.getByRole('button', { name: '下一页', exact: true })).toHaveCount(0);
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
  await page.getByRole('button', { name: 'Automatische Stile', exact: true }).click();
  await settleNavigation(page);
  const picker = await revealField(page, 'Stilpaket für Notes');
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
  const surface = page.locator('.ol-app-style-page');
  expect(await surface.evaluate((node) => node.scrollHeight <= node.clientHeight + 1)).toBe(true);
  const left = surface.locator('.ol-app-style-panel-body').first();
  expect(await left.evaluate((node) => node.scrollHeight <= node.clientHeight + 1)).toBe(true);
});

test('renews the catalog automatically on page entry and restored focus', async ({ page }) => {
  let requests = 0;
  await page.route('https://apic.openless.top/v1/app-catalog', (route) => {
    requests++;
    return route.fulfill({ json: catalog });
  });
  await openPage(page);
  await expect.poll(() => requests).toBeGreaterThan(0);
  let before = requests;
  await page.evaluate(() => window.dispatchEvent(new Event('focus')));
  await expect.poll(() => requests).toBeGreaterThan(before);
  before = requests;
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect.poll(() => requests).toBeGreaterThan(before);
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await settleNavigation(page);
  before = requests;
  await page.getByRole('button', { name: '自动风格', exact: true }).click();
  await settleNavigation(page);
  await expect.poll(() => requests).toBeGreaterThan(before);
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
