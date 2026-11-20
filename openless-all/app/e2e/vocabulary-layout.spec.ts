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
    const selectAll = vocab.getByRole('button', {
      name: narrow ? 'Sélectionner tous les résultats' : '选择所有结果',
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
    if (!narrow) expect(filtersBefore.x).toBeGreaterThanOrEqual(addBefore.x + addBefore.width);
    const stableLayout = () =>
      Promise.all([
        bounds(filters),
        bounds(wordList),
        bounds(firstWord),
        bounds(firstWord.locator('.ol-word-card-label')),
        bounds(firstWord.locator('.ol-word-card-hits')),
      ]);
    const layout = await stableLayout();
    await expect(remove).toHaveCount(0);
    await expect(selectAll).toHaveCount(0);

    await select.click();
    await expect(done).toHaveCSS('transition-property', 'inline-size, border-radius');
    await expect
      .poll(async () => {
        const rect = await bounds(done);
        return Math.abs(rect.width - rect.height);
      })
      .toBeLessThan(0.5);
    const close = await bounds(done);
    expect(close.x + close.width).toBeCloseTo(selectBefore.x + selectBefore.width, 0);
    await expect(done).toHaveCSS('border-radius', '18px');
    await expect(remove).toBeVisible();
    await expect(remove).toBeDisabled();
    await expect(selectAll).toBeVisible();
    const idleTrash = await bounds(remove);
    const idleColor = await remove.locator('svg').evaluate((icon) => getComputedStyle(icon).color);
    expect(await stableLayout()).toEqual(layout);

    const firstCheckbox = firstWord.getByRole('checkbox');
    await firstCheckbox.click();
    await expect(firstCheckbox).toBeChecked();
    const badge = firstWord.locator('.ol-word-card-badge');
    const mark = await bounds(badge);
    const word = await bounds(firstWord);
    expect(Math.abs(mark.x + mark.width / 2 - (word.x + word.width))).toBeLessThan(5);
    expect(Math.abs(mark.y + mark.height / 2 - word.y)).toBeLessThan(5);
    await expect(remove).toHaveAccessibleName(
      narrow ? 'Supprimer la sélection (1)' : '删除已选（1）',
    );
    const all = await bounds(selectAll);
    const trash = await bounds(remove);
    expect(trash).toEqual(idleTrash);
    expect(all.x + all.width).toBeLessThan(trash.x);
    expect(all.y + all.height / 2).toBeCloseTo(trash.y + trash.height / 2, 0);
    expect(trash.y).toBeGreaterThan(word.y);
    await expect(remove).toBeEnabled();
    await expect(remove.locator('svg')).not.toHaveCSS('color', idleColor);
    expect(await stableLayout()).toEqual(layout);

    await firstCheckbox.click();
    await expect(firstCheckbox).not.toBeChecked();
    await expect(badge).toHaveCount(0);
    await expect(remove).toBeDisabled();
    await expect(remove.locator('svg')).toHaveCSS('color', idleColor);
    expect(await stableLayout()).toEqual(layout);

    if (!narrow) {
      await firstCheckbox.click();
      const choices = wordList.getByRole('checkbox');
      await choices.nth(3).click({ modifiers: ['Shift'] });
      await expect(wordList.locator('[aria-checked="true"]')).toHaveCount(4);
      await done.hover();
      await page.mouse.down();
      await expect(selectAll).toBeDisabled();
      await page.mouse.up();
      await expect(done).toHaveAccessibleName('完成');
      await expect(choices.last()).toBeChecked();
      expect(await stableLayout()).toEqual(layout);
    }

    await done.click();
    await expect(remove).toHaveCount(0);
    await expect(selectAll).toHaveCount(0);
    expect(await stableLayout()).toEqual(layout);
    await search.click();
    const input = vocab.getByRole('textbox', { name: narrow ? 'Rechercher' : '搜索' });
    await expect(input).toBeFocused();
    await expect.poll(async () => (await bounds(input)).width).toBeGreaterThan(30);
    expect((await bounds(input)).x).toBeGreaterThan(searchBefore.x);
    expect((await bounds(search)).x).toBeCloseTo(searchBefore.x, 0);
  });
}

test('five vocabulary cards stay fixed while a long label loops on whole-card hover', async ({
  page,
}) => {
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

  const phrase = 'TypeLazyWordsFormWithAnExtraLongName';
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
  await card.locator('.ol-word-card-icon').hover();
  await expect(label).toHaveAttribute('data-scrolling', '');
  const loop = await label.locator('.ol-word-card-strip').evaluate((node: HTMLElement) => {
    const copy = node.lastElementChild!;
    const first = node.firstElementChild!;
    const animation = node.getAnimations()[0];
    const effect = animation?.effect as KeyframeEffect;
    const { duration, iterations } = effect.getTiming();
    animation.pause();
    animation.currentTime = 0;
    const origin = first.getBoundingClientRect().x;
    animation.currentTime = Number(duration) - 1;
    const firstPass = copy.getBoundingClientRect().x;
    animation.currentTime = 2 * Number(duration) - 1;
    const secondPass = copy.getBoundingClientRect().x;
    return {
      loops: iterations === Infinity,
      firstOffset: firstPass - origin,
      secondOffset: secondPass - origin,
    };
  });
  expect(loop.loops).toBe(true);
  expect(loop.firstOffset).toBeCloseTo(0, 0);
  expect(loop.secondOffset).toBeCloseTo(0, 0);
  expect((await bounds(card)).width).toBeCloseTo(before.width, 0);
  expect((await bounds(card)).height).toBeCloseTo(before.height, 0);
  await vocab.getByRole('heading').hover();
  await expect(label).not.toHaveAttribute('data-scrolling', '');
  await longWord.press('Shift');
  await expect(label).toHaveAttribute('data-scrolling', '');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(label).not.toHaveAttribute('data-scrolling', '');
  await expect(label).toHaveCSS('text-overflow', 'ellipsis');
});

test('holding a vocabulary card opens the delete menu without toggling or deleting the word', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '词典', exact: true }).click();
  const vocab = page.locator('[data-ol-page="vocab"]');
  await expect(vocab).toHaveAttribute('data-transition-phase', 'settled');
  const word = vocab.getByRole('button', { name: 'macOS', exact: true });
  const card = word.locator('..');
  const hits = card.locator('.ol-word-card-hits');
  await expect(hits).toHaveText('8');
  const normal = await bounds(hits);
  const originalCard = await bounds(card);
  const label = await bounds(card.locator('.ol-word-card-label'));
  const edit = await bounds(card.getByRole('button', { name: '编辑', exact: true }));
  await expect(hits).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(hits).toHaveCSS('border-radius', '0px');
  expect(normal.x).toBeGreaterThan(label.x + label.width);
  expect(normal.x + normal.width).toBeLessThan(edit.x);
  const wide = await hits.evaluate((node) => {
    const original = node.textContent;
    node.textContent = '1234';
    const { x, y, width, height } = node.getBoundingClientRect();
    node.textContent = original;
    return { x, y, width, height };
  });
  expect(wide.x + wide.width).toBeCloseTo(normal.x + normal.width, 0);
  expect(wide.y).toBeCloseTo(normal.y, 0);
  expect(wide.height).toBeCloseTo(normal.height, 0);
  expect(wide.width).toBeGreaterThan(normal.width);
  expect(await bounds(card)).toEqual(originalCard);
  await expect(card.getByRole('button', { name: /删除/ })).toHaveCount(0);
  await expect(card.getByRole('button', { name: '编辑', exact: true })).toBeVisible();

  await card.locator('.ol-word-card-hits').hover();
  await page.mouse.down();
  const menu = page.getByRole('menu', { name: 'macOS' });
  await expect(menu).toBeVisible();
  const popup = await bounds(menu);
  const tile = await bounds(card);
  expect(popup.y).toBeGreaterThan(tile.y + tile.height);
  await page.mouse.up();
  await expect(card).not.toHaveAttribute('data-disabled', 'true');
  await expect(page.getByRole('dialog')).toHaveCount(0);

  await menu.getByRole('menuitem', { name: '删除' }).click();
  const confirmation = page.getByRole('dialog', { name: /删除.*macOS/ });
  await confirmation.getByRole('button', { name: '取消', exact: true }).click();
  await expect(word).toBeFocused();
  await word.press('Shift+F10');
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitem', { name: '删除' }).click();
  await confirmation.getByRole('button', { name: '删除', exact: true }).click();
  await expect(word).toHaveCount(0);
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
