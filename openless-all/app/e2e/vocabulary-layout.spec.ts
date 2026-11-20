import { expect, test, type Locator } from '@playwright/test';

async function bounds(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error('Expected a visible element with a layout box');
  return box;
}

for (const narrow of [false, true]) {
  test(`vocabulary actions stay aligned and preserve layout · ${narrow ? 'narrow French' : 'desktop'}`, async ({
    page,
  }) => {
    if (narrow) {
      await page.setViewportSize({ width: 320, height: 800 });
      await page.addInitScript(() => localStorage.setItem('ol.locale', 'fr'));
    }
    await page.goto('/?os=mac');
    const nav = page.locator(narrow ? '.ol-mobile-bottom-nav' : '.ol-sidebar-surface nav');
    if (narrow) {
      await nav.getByRole('button', { name: 'Plus', exact: true }).click();
      await page
        .getByRole('dialog', { name: 'Plus', exact: true })
        .getByRole('button', { name: 'Dictionnaire', exact: true })
        .click();
      await expect(page.locator('.ol-mobile-navigation-sheet')).toHaveCount(0);
    } else {
      await nav.getByRole('button', { name: '词典', exact: true }).click();
    }

    const vocab = page.locator('[data-ol-page="vocab"]');
    await expect(vocab).toHaveAttribute('data-transition-phase', 'settled');
    const add = vocab.getByRole('button', { name: narrow ? 'Nouveau mot' : '新词', exact: true });
    const remove = vocab.getByRole('button', {
      name: narrow ? /^Supprimer la sélection/ : /^删除已选/,
    });
    const search = vocab.getByRole('button', {
      name: narrow ? 'Rechercher' : '搜索',
      exact: true,
    });
    const filters = vocab.getByRole('tablist');
    const select = vocab.getByRole('button', {
      name: narrow ? 'Sélectionner' : '选择',
      exact: true,
    });
    const done = vocab.locator('.ol-vocab-select-mode');
    const wordList = vocab.locator('.ol-thinscroll');
    const firstWord = wordList.locator('.ol-word-card').first();
    await expect(firstWord).toBeVisible();

    const addBefore = await bounds(add);
    const searchBefore = await bounds(search);
    const filtersBefore = await bounds(filters);
    const selectBefore = await bounds(select);
    expect(addBefore.width).toBeCloseTo(searchBefore.width, 0);
    expect(addBefore.height).toBeCloseTo(searchBefore.height, 0);
    await expect(add).toHaveText('');
    await expect(add).toHaveCSS(
      'background-color',
      await search.locator('..').evaluate((node) => getComputedStyle(node).backgroundColor),
    );
    expect(addBefore.x).toBeGreaterThanOrEqual(searchBefore.x + searchBefore.width);
    expect(selectBefore.x).toBeGreaterThanOrEqual(filtersBefore.x + filtersBefore.width);
    expect(selectBefore.height).toBeCloseTo(filtersBefore.height, 0);
    expect(selectBefore.y).toBeCloseTo(filtersBefore.y, 0);
    if (!narrow) expect(filtersBefore.x).toBeGreaterThanOrEqual(addBefore.x + addBefore.width);
    await select.click();
    expect((await bounds(done)).height).toBeCloseTo(selectBefore.height, 0);
    expect((await bounds(done)).width).toBeCloseTo(selectBefore.width, 0);
    await expect(remove).toHaveCount(0);
    const layout = async () =>
      Promise.all([bounds(add), bounds(filters), bounds(search), bounds(wordList)]);
    const unselected = await layout();
    expect(unselected[0]).toEqual(addBefore);

    const rightEdge = page.viewportSize()!.width;
    expect(unselected[1].x + unselected[1].width).toBeLessThanOrEqual(rightEdge);
    const firstCheckbox = firstWord.getByRole('checkbox');
    await firstCheckbox.check();
    await expect(remove).toHaveAccessibleName(
      narrow ? 'Supprimer la sélection (1)' : '删除已选（1）',
    );
    const a = await bounds(add);
    const b = await bounds(remove);
    expect(a).toEqual(addBefore);
    expect(a.y + a.height / 2).toBeCloseTo(b.y + b.height / 2, 0);
    expect(b.x).toBeGreaterThanOrEqual(0);
    expect(a.x + a.width).toBeLessThanOrEqual(rightEdge);
    for (const button of [add, remove]) {
      const offset = await button.evaluate((el) => {
        const box = el.getBoundingClientRect();
        const children = Array.from(el.children)
          .filter((child) => getComputedStyle(child).visibility === 'visible')
          .map((child) => child.getBoundingClientRect());
        return {
          x: (children[0].left + children[children.length - 1].right - box.left - box.right) / 2,
          y:
            (Math.min(...children.map((child) => child.top)) +
              Math.max(...children.map((child) => child.bottom)) -
              box.top -
              box.bottom) /
            2,
        };
      });
      expect(offset.x).toBeCloseTo(0, 0);
      expect(offset.y).toBeCloseTo(0, 0);
    }
    expect(await layout()).toEqual(unselected);

    await vocab.locator('.ol-vocab-select-all input').check();
    await expect(remove).not.toHaveAccessibleName(narrow ? /\(1\)$/ : /（1）$/);
    expect(await layout()).toEqual(unselected);
    await vocab.locator('.ol-vocab-select-all input').uncheck();
    await expect(remove).toHaveCount(0);
    expect(await layout()).toEqual(unselected);

    await done.click();
    await search.click();
    const input = vocab.getByRole('textbox', { name: narrow ? 'Rechercher' : '搜索' });
    await expect(input).toBeFocused();
    await expect.poll(async () => (await bounds(input)).width).toBeGreaterThan(30);
    expect((await bounds(input)).x).toBeGreaterThan(searchBefore.x);
    expect((await bounds(search)).x).toBeCloseTo(searchBefore.x, 0);
  });
}

test('vocabulary fits five cards and scrolls only the overflowing phrase', async ({ page }) => {
  await page.setViewportSize({ width: 1300, height: 835 });
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '词典', exact: true }).click();
  const vocab = page.locator('[data-ol-page="vocab"]');
  await expect(vocab).toHaveAttribute('data-transition-phase', 'settled');
  const cards = vocab.locator('.ol-word-card');
  await expect(cards.nth(5)).toBeVisible();
  const grid = vocab.locator('.ol-vocab-grid');
  expect(
    await grid.evaluate((node) => getComputedStyle(node).gridTemplateColumns.split(' ').length),
  ).toBe(5);
  const first = await bounds(cards.nth(0));
  const fifth = await bounds(cards.nth(4));
  expect(first.y).toBeCloseTo(fifth.y, 0);
  expect(first.width).toBeCloseTo(fifth.width, 0);
  expect((await bounds(cards.nth(5))).y).toBeGreaterThan(first.y);

  const phrase = '一条非常长的词条用于验证收窄之后仍然能够完整查看其中的所有文字';
  await vocab.getByRole('button', { name: '新词' }).click();
  const dialog = page.getByRole('dialog', { name: '添加新词' });
  await dialog.getByRole('textbox').fill(phrase);
  await dialog.getByRole('button', { name: '添加', exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  const longWord = vocab.getByRole('button', { name: phrase, exact: true });
  const label = longWord.locator('.ol-word-card-label');
  const card = longWord.locator('..');
  const before = await bounds(card);
  await expect(label).toHaveCSS('text-overflow', 'ellipsis');
  await longWord.focus();
  await expect(label).toHaveAttribute('data-scrolling', '');
  const delta = await label.locator('.ol-word-card-phrase').evaluate((node: HTMLElement) => {
    const animation = node.getAnimations()[0];
    const effect = animation?.effect as KeyframeEffect;
    const frames = effect.getKeyframes();
    animation.pause();
    animation.currentTime = frames[2].offset! * Number(effect.getTiming().duration);
    const actual = new DOMMatrix(getComputedStyle(node).transform).m41;
    return { actual, expected: node.parentElement!.clientWidth - node.offsetWidth };
  });
  expect(delta.expected).toBeLessThan(0);
  expect(delta.actual).toBeCloseTo(delta.expected, 0);
  expect((await bounds(card)).width).toBeCloseTo(before.width, 0);
  expect((await bounds(card)).height).toBeCloseTo(before.height, 0);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(label).not.toHaveAttribute('data-scrolling', '');
  await expect(label).toHaveCSS('text-overflow', 'ellipsis');
});

test('add-word dialog keeps one blur through dismissal and retains the input focus ring', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page
    .locator('.ol-sidebar-surface nav')
    .getByRole('button', { name: '词典', exact: true })
    .click();
  const vocab = page.locator('[data-ol-page="vocab"]');
  await expect(vocab).toHaveAttribute('data-transition-phase', 'settled');
  await expect(page.locator('html')).toHaveAttribute('data-ol-theme', 'dark');
  await vocab.getByRole('button', { name: '新词', exact: true }).click();

  const dialog = page.getByRole('dialog', { name: '添加新词' });
  await expect(dialog).toBeVisible();
  const overlay = dialog.locator('..');
  const content = page.locator('.ol-viewport-content');
  await expect(overlay).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(overlay).toHaveCSS('backdrop-filter', 'none');
  await expect(content).toHaveCSS('filter', 'blur(6px)');
  expect(
    await overlay.evaluate((node) => ({
      overlayAnimations: node.getAnimations().length,
      extraBackdrop: getComputedStyle(node, '::before').content,
    })),
  ).toEqual({ overlayAnimations: 0, extraBackdrop: 'none' });
  const shadow = await dialog.evaluate((el) => getComputedStyle(el).boxShadow);
  expect(shadow).not.toBe('none');
  expect(shadow).not.toContain('inset');
  const input = dialog.getByRole('textbox');
  await input.focus();
  await expect(input).toBeFocused();
  const frame = dialog.locator('.ol-field:focus-within');
  await expect(frame).toHaveCSS(
    'border-color',
    await frame.evaluate((el) => {
      const swatch = document.createElement('span');
      swatch.style.color = 'var(--ol-blue)';
      el.append(swatch);
      const color = getComputedStyle(swatch).color;
      swatch.remove();
      return color;
    }),
  );
  const dismiss = dialog.getByRole('button', { name: '关闭', exact: true }).first();
  const filters = await dismiss.evaluate((button: HTMLElement) => {
    const overlay = button.closest('.ol-dialog-overlay')!;
    const content = document.querySelector('.ol-viewport-content')!;
    return new Promise<string[]>((resolve) => {
      const samples = [getComputedStyle(content).filter];
      const sample = () => {
        samples.push(getComputedStyle(content).filter);
        if (overlay.isConnected) requestAnimationFrame(sample);
        else resolve(samples);
      };
      button.click();
      requestAnimationFrame(sample);
    });
  });
  expect(filters.length).toBeGreaterThan(2);
  expect(filters.slice(0, -1).every((filter) => filter === 'blur(6px)')).toBe(true);
  expect(filters.at(-1)).toBe('none');
});
