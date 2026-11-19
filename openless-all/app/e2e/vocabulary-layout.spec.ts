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
    const wordList = vocab.locator('.ol-thinscroll');
    const firstWord = wordList.locator('.ol-word-card').first();
    await expect(firstWord).toBeVisible();

    const addBefore = await bounds(add);
    await vocab
      .getByRole('button', { name: narrow ? 'Sélectionner' : '选择', exact: true })
      .click();
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
    expect(a.width).toBeCloseTo(b.width, 0);
    expect(a.height).toBeCloseTo(b.height, 0);
    expect(a.y).toBeCloseTo(b.y, 0);
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
  });
}

test('dark add-word dialog uses a muted card shadow and retains the input focus ring', async ({
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
});
