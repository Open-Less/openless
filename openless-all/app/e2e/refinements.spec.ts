import { test, expect, type Page } from '@playwright/test';

const fixture = 'http://127.0.0.1:5326/e2e/fixtures/refinements.html';
async function openShortcuts(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
}

test('shortcut search and categories live in the flat settings pane; keyboard close preserves exit', async ({
  page,
}) => {
  await openShortcuts(page);
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(dialog).toHaveCSS('border-radius', '0px');
  expect(await dialog.locator('.ol-shortcuts .ol-card').count()).toBe(0);
  const search = dialog.getByRole('searchbox', { name: '搜索命令或按键' });
  await expect(search).toHaveCSS('opacity', '1');
  await search.fill('速记');
  await expect(dialog.locator('.ol-shortcut-row')).toHaveCount(1);
  await search.fill('NoSuchCommand');
  await expect(dialog.getByText('没有匹配的命令')).toBeVisible();
  await search.fill('');
  await dialog
    .getByRole('navigation', { name: '分类' })
    .getByRole('button', { name: '系统', exact: true })
    .click();
  await expect(dialog.locator('.ol-shortcut-row[role="group"]')).toHaveCount(1);
  await expect(dialog.locator('.ol-shortcut-row[role="group"]')).toContainText('打开 OpenLess');
  await expect(dialog.getByText('Less Computer', { exact: true })).toHaveCount(0);
  await dialog.getByRole('button', { name: '返回应用', exact: true }).press('Enter');
  await expect(dialog).toHaveCount(0);
  await page.getByRole('button', { name: '设置', exact: true }).press('Enter');
  await expect(dialog).toBeVisible();
  await expect(page.locator('.ol-settings-workspace')).toHaveCSS('opacity', '1');
});

test('native scrolling has no JS wheel interception and is stable when stopped', async ({
  page,
}) => {
  // The wheel distance is asserted in CSS pixels, which match device pixels only at 100%.
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
  await openShortcuts(page);
  const content = page.locator('.ol-settings-scroll');
  await expect(content).toHaveCSS('scroll-behavior', 'auto');
  await expect(content).toHaveCSS('scrollbar-gutter', 'stable');
  await content.evaluate((el) => {
    const event = new WheelEvent('wheel', { deltaY: 100, bubbles: true, cancelable: true });
    el.dispatchEvent(event);
    el.setAttribute('data-wheel-prevented', String(event.defaultPrevented));
  });
  await expect(content).toHaveAttribute('data-wheel-prevented', 'false');
  await page
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name: '录音与输入', exact: true })
    .click();
  await expect(
    content.locator('[data-section="general"] .ol-shortcut-binding').first(),
  ).toBeVisible();
  await content.hover();
  const destination = await content.evaluate((el) =>
    Math.min(el.scrollTop + 450, el.scrollHeight - el.clientHeight),
  );
  await page.mouse.wheel(0, 450);
  // A single unchanged 120ms sample can still be WebKit's intermediate 11px
  // compositor position. Await the complete clamped native wheel displacement
  // before asserting rest; this is stronger than accepting any positive offset.
  await expect.poll(() => content.evaluate((el) => el.scrollTop)).toBe(destination);
  const stopped = await content.evaluate((el) => el.scrollTop);
  await page.waitForTimeout(160);
  expect(await content.evaluate((el) => el.scrollTop)).toBe(stopped);
});

test('compact typography selects persist font, size and reading width without network fonts', async ({
  page,
}) => {
  const fonts: string[] = [];
  page.on('request', (request) => {
    if (request.resourceType() === 'font') fonts.push(request.url());
  });
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '外观与语言', exact: true }).click();
  const section = page.locator('.ol-typography');
  await expect(section.getByRole('combobox')).toHaveCount(3);
  await expect(section.getByText('文字用途', { exact: true })).toHaveCount(0);
  await expect(
    section.locator(
      '.ol-font-options, .ol-typography-preview, input[type="search"], input[type="file"]',
    ),
  ).toHaveCount(0);
  expect((await section.boundingBox())!.height).toBeLessThan(280);
  await section.getByRole('combobox', { name: '字体系列' }).click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await expect(section.getByRole('listbox')).toHaveCount(0);
  await page.getByRole('option', { name: 'Avenir Next · Aptos', exact: true }).click();
  await expect(section.getByRole('combobox', { name: '字体系列' })).toBeFocused();
  await section.getByRole('combobox', { name: '阅读宽度' }).press('Enter');
  await page.getByRole('option', { name: '聚焦', exact: true }).click();
  await expect(page.locator('html')).toHaveCSS('--ol-reading-width', '56ch');
  await section.getByRole('combobox', { name: '界面大小' }).click();
  await page.getByRole('option', { name: '110%', exact: true }).click();
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('ol.typography.v1')!));
  expect(stored.fonts).toEqual({
    ui: 'humanist',
    body: 'humanist',
    title: 'humanist',
    numbers: 'humanist',
    code: 'mono',
  });
  await page.reload();
  await expect(page.locator('html')).toHaveCSS('--ol-reading-width', '56ch');
  await expect(page.locator('html')).toHaveCSS('zoom', '1.1');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '外观与语言', exact: true }).click();
  await expect(section.getByRole('combobox', { name: '字体系列' })).toHaveText(
    'Avenir Next · Aptos',
  );
  await section.getByRole('combobox', { name: '字体系列' }).click();
  await page.getByRole('option', { name: '系统默认', exact: true }).click();
  await expect(page.locator('html')).not.toHaveAttribute(
    'style',
    /--ol-font-(sans|body|display|numeric):/,
  );
  expect(fonts).toEqual([]);
});

test('startup has no video and records real navigation-relative milestones', async ({ page }) => {
  const videos: string[] = [];
  page.on('request', (request) => {
    if (/\.mp4/.test(request.url())) videos.push(request.url());
  });
  await page.goto('/');
  await expect(page.getByRole('button', { name: '设置', exact: true })).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => performance.getEntriesByName('openless:interactive').length))
    .toBe(1);
  const marks = await page.evaluate(() =>
    performance
      .getEntriesByType('mark')
      .filter((m) => m.name.startsWith('openless:'))
      .map((m) => ({ name: m.name, at: m.startTime })),
  );
  expect(marks.find((m) => m.name === 'openless:html')!.at).toBeLessThan(
    marks.find((m) => m.name === 'openless:interactive')!.at,
  );
  expect(videos).toEqual([]);
  await expect(page.locator('video')).toHaveCount(0);
});

test('every main destination and settings category stays readable on desktop and narrow windows', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
  for (const name of ['历史', '词典']) {
    await page.getByRole('button', { name, exact: true }).click();
    await expect(page.locator('main h1')).toBeVisible();
  }
  await page.getByRole('button', { name: '风格', exact: true }).click();
  for (const name of ['润色模式', '风格市场']) {
    await page.getByRole('button', { name, exact: true }).click();
    await expect(page.locator('main h1')).toBeVisible();
  }
  await page.getByRole('button', { name: '工具', exact: true }).click();
  for (const name of ['翻译', '问任何问题', '速记', '纠正规则']) {
    await page.getByRole('button', { name, exact: true }).click();
    await expect(page.locator('main h1')).toBeVisible();
  }
  await page.getByRole('button', { name: '设置', exact: true }).click();
  for (const name of [
    '录音与输入',
    '快捷键',
    'AI 服务与模型',
    '外观与语言',
    '权限与数据',
    '实验与扩展',
    '关于与更新',
  ]) {
    await page
      .getByRole('navigation', { name: '设置分类' })
      .getByRole('button', { name, exact: true })
      .click();
    await expect(
      page
        .getByRole('dialog', { name: '设置', exact: true })
        .getByRole('heading', { name, exact: true })
        .first(),
    ).toBeVisible();
  }
  await page.setViewportSize({ width: 375, height: 812 });
  await page.getByRole('button', { name: '快捷键', exact: true }).click();
  const scroll = page.locator('.ol-settings-scroll');
  expect(await scroll.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: 'test-results/refinements-mobile.png' });
  expect(errors).toEqual([]);
});

test('dragging makes a fractional gap, follows the pointer, reverses, cancels and keyboard reorders', async ({
  page,
}) => {
  await page.goto(fixture);
  const row = page.locator('[data-row="Beta"]');
  const handle = page.getByRole('button', { name: 'Beta', exact: true });
  const rect = (await handle.boundingBox())!;
  const rowTop = (await row.boundingBox())!.y;
  const third = page.locator('[data-row="Gamma"]');
  const initial = (await third.boundingBox())!.y;
  await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2);
  await page.mouse.down();
  await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2 + 36, { steps: 4 });
  await page.waitForTimeout(200);
  const half = (await third.boundingBox())!.y;
  expect(initial - half).toBeGreaterThan(12);
  expect(initial - half).toBeLessThan(62);
  expect(Math.abs((await row.boundingBox())!.y - rowTop - 36)).toBeLessThan(1);
  await page.mouse.move(rect.x + rect.width / 2, rect.y + rect.height / 2, { steps: 4 });
  await page.keyboard.press('Escape');
  await page.mouse.up();
  await expect(page.locator('[data-reordering]')).toHaveCount(0);
  await expect(page.getByTestId('commits')).toHaveText('0');
  expect(
    await page
      .locator('[data-row]')
      .evaluateAll((elements) => elements.map((el) => el.getAttribute('data-row'))),
  ).toEqual(['Alpha', 'Beta', 'Gamma', 'Delta']);
  await handle.press('ArrowDown');
  await expect(page.getByTestId('commits')).toHaveText('1');
  expect(
    await page
      .locator('[data-row]')
      .evaluateAll((elements) => elements.map((el) => el.getAttribute('data-row'))),
  ).toEqual(['Alpha', 'Gamma', 'Beta', 'Delta']);
});

test('switch ripple never changes sibling state; keyboard and reduced motion suppress it', async ({
  page,
}) => {
  await page.goto(fixture);
  await page.getByRole('switch', { name: 'Switch 0' }).click();
  await expect(page.getByRole('switch', { name: 'Switch 0' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await expect(page.getByRole('switch', { name: 'Switch 1' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await expect(page.getByRole('switch', { name: 'Switch 2' })).toHaveAttribute(
    'aria-checked',
    'false',
  );
  await page.waitForTimeout(350);
  await page.getByRole('switch', { name: 'Switch 0' }).press('Enter');
  expect(
    await page
      .getByRole('switch', { name: 'Switch 1' })
      .evaluate((el) => el.getAnimations().length),
  ).toBe(0);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('switch', { name: 'Switch 0' }).click();
  expect(
    await page
      .getByRole('switch', { name: 'Switch 1' })
      .evaluate((el) => el.getAnimations().length),
  ).toBe(0);
});

test('text height grows with contents and chips give the selection actual layout space', async ({
  page,
}) => {
  await page.goto(fixture);
  const clip = page.locator('.ol-expandable-text-clip');
  const initial = (await clip.boundingBox())!.height;
  await page.getByRole('button', { name: '展开全文' }).click();
  await expect(page.getByRole('button', { name: '收起' })).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(async () => (await clip.boundingBox())!.height).toBeGreaterThan(initial * 3);
  // This assertion concerns layout space, not the temporary FLIP/scale
  // transforms applied by Framer Motion (which WebKit samples fractionally).
  const before = await page
    .getByRole('button', { name: 'Three', exact: true })
    .evaluate((el) => (el as HTMLElement).offsetLeft);
  await page.getByRole('button', { name: 'Two', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Two', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  const selected = await page
    .getByRole('button', { name: 'Two', exact: true })
    .evaluate((el) => ({
      x: (el as HTMLElement).offsetLeft,
      width: (el as HTMLElement).offsetWidth,
    }));
  const last = await page
    .getByRole('button', { name: 'Three', exact: true })
    .evaluate((el) => (el as HTMLElement).offsetLeft);
  expect(selected.x + selected.width).toBeLessThan(last);
  expect(last).toBeGreaterThanOrEqual(before - 1);
});

test('swipe departure is directional, removable and undoable', async ({ page }) => {
  await page.goto(fixture);
  const card = page.getByTestId('card');
  await card.scrollIntoViewIfNeeded();
  const rect = (await card.boundingBox())!;
  await page.mouse.move(rect.x + 100, rect.y + 70);
  await page.mouse.down();
  await page.mouse.move(rect.x + 270, rect.y + 70, { steps: 12 });
  await expect
    .poll(() =>
      card.evaluate((element) => new DOMMatrixReadOnly(getComputedStyle(element).transform).m41),
    )
    .toBeGreaterThan(120);
  await page.mouse.up();
  await expect(card).toHaveCount(0);
  await page.getByRole('button', { name: 'Undo', exact: true }).click();
  await expect(card).toBeVisible();
});
