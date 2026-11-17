import { expect, test, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test.use({ baseURL: fixtureOrigin });

type Box = { left: number; right: number; top: number };

async function boxes(page: Page, id: string): Promise<Box[]> {
  return page.locator(`#${id}`).evaluate((grid) => {
    const origin = grid.getBoundingClientRect();
    return [...grid.children].map((child) => {
      const box = child.getBoundingClientRect();
      return {
        left: box.left - origin.left,
        right: box.right - origin.left,
        top: box.top - origin.top,
      };
    });
  });
}

async function trackCount(page: Page, id: string) {
  return page.locator(`#${id}`).evaluate((grid) => {
    return getComputedStyle(grid).gridTemplateColumns.split(' ').filter(Boolean).length;
  });
}

function near(actual: number, expected: number, label = '') {
  expect(Math.abs(actual - expected), label).toBeLessThanOrEqual(1);
}

test('content rows share the same 12 column edges', async ({ page }) => {
  await page.goto('/e2e/fixtures/grid-12.html');
  await expect(page.locator('#reference')).toBeVisible();

  const columns = await boxes(page, 'reference');
  expect(columns).toHaveLength(12);
  expect(await trackCount(page, 'reference')).toBe(12);
  const width = columns[11].right;
  for (const id of [
    'cards',
    'stats',
    'quarters',
    'meta',
    'fields',
    'form',
    'pipeline',
    'bands',
    'composer',
    'split',
  ]) {
    const row = await boxes(page, id);
    near(row[0].left, columns[0].left, `${id} left`);
    expect(await trackCount(page, id)).toBe(12);
    if (id !== 'bands') near(row[row.length - 1].right, width, `${id} right`);
  }

  const cards = await boxes(page, 'cards');
  near(cards[1].left, columns[4].left);
  near(cards[2].left, columns[8].left);

  const stats = await boxes(page, 'stats');
  const quarters = await boxes(page, 'quarters');
  for (let index = 0; index < 4; index += 1) {
    near(stats[index].left, columns[index * 3].left);
    near(quarters[index].left, stats[index].left);
  }

  const meta = await boxes(page, 'meta');
  near(meta[1].left, columns[4].left);
  near(meta[2].left, columns[8].left);

  const fields = await boxes(page, 'fields');
  near(fields[1].left, columns[6].left);

  const form = await boxes(page, 'form');
  near(form[1].left, columns[3].left);

  const pipeline = await boxes(page, 'pipeline');
  near(pipeline[1].left, columns[3].left);
  near(pipeline[2].left, columns[9].left);
  near(pipeline[3].left, columns[0].left);

  const bands = await boxes(page, 'bands');
  near(bands[1].left, columns[2].left);
  near(bands[5].right, width);
  near(bands[6].left, columns[0].left);
  near(bands[9].right, columns[7].right);
  expect(bands[6].top).toBeGreaterThan(bands[0].top);

  const composer = await boxes(page, 'composer');
  near(composer[1].left, columns[5].left);
  near(composer[2].left, columns[6].left);
  near(composer[3].left, columns[10].left);

  const split = await boxes(page, 'split');
  near(split[1].left, columns[5].left);

  const forced = await boxes(page, 'forced-one');
  expect(await trackCount(page, 'forced-one')).toBe(1);
  near(forced[0].left, 0);
  near(forced[0].right, width);

  await page.evaluate(() => {
    document.documentElement.dataset.olStackedLayout = 'true';
  });
  const stacked = await boxes(page, 'stacked-cards');
  expect(await trackCount(page, 'stacked-cards')).toBe(1);
  for (const item of stacked) {
    near(item.left, 0);
    near(item.right, width);
  }
});
