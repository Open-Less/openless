import { expect, test, type Page } from '@playwright/test';

test.use({ baseURL: 'http://127.0.0.1:5326' });

/** Import the exact module instance the app loaded (Vite appends an HMR query). */
async function emitNotifications(
  page: Page,
  items: {
    state: 'saving' | 'saved' | 'deleted' | 'failed' | 'info';
    message: string;
    durationMs?: number;
    progress?: boolean;
  }[],
) {
  await page.evaluate(async (items) => {
    const url = performance
      .getEntriesByType('resource')
      .map((entry) => entry.name)
      .find((name) => name.includes('/src/lib/notifications.ts'));
    const { notify } = await import(url || '/src/lib/notifications.ts');
    for (const item of items) notify(item);
  }, items);
}

test('notifications stack above settings, preserve errors and support dismissal', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await emitNotifications(page, [
    { state: 'saved', message: 'First notification', durationMs: 10000 },
    { state: 'failed', message: 'Second notification' },
  ]);
  const cards = page.locator('.ol-notification-card');
  await expect(cards).toHaveCount(2);
  await expect(page.getByRole('alert').filter({ hasText: 'Second notification' })).toBeVisible();
  const center = await cards.first().boundingBox();
  expect(Math.abs(center!.x + center!.width / 2 - 650)).toBeLessThan(2);
  await cards.last().getByRole('button').click();
  await expect(cards).toHaveCount(1);
});

test('transient confirmations drop from the window top and replace each other', async ({
  page,
}) => {
  await page.goto('/?os=mac');
  await emitNotifications(page, [
    { state: 'saved', message: 'First saved', durationMs: 10000 },
    { state: 'saved', message: 'Second saved', durationMs: 10000 },
  ]);
  const cards = page.locator('.ol-notification-card');
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toContainText('Second saved');
  const box = (await cards.first().boundingBox())!;
  // The host is flush with the window edge; the card itself keeps only the safe-area inset.
  expect(box.y).toBeLessThan(12);

  // Success uses the standard theme blue and the same raised surface as content cards.
  const savedIcon = page.locator('.ol-notification-icon--saved');
  const savedBg = await savedIcon.evaluate((node) => getComputedStyle(node).backgroundColor);
  const themeBlue = await page.evaluate(() => {
    const probe = document.createElement('div');
    probe.style.background = 'var(--ol-accent-solid-bg)';
    document.body.appendChild(probe);
    const value = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return value;
  });
  expect(savedBg).toBe(themeBlue);
  const cardBackground = await cards
    .first()
    .evaluate((node) => getComputedStyle(node).backgroundColor);
  const contentCardBackground = await page
    .locator('.ol-overview-dashboard .ol-card')
    .first()
    .evaluate((node) => getComputedStyle(node).backgroundColor);
  expect(cardBackground).toBe(contentCardBackground);

  await emitNotifications(page, [{ state: 'deleted', message: '已删除', durationMs: 10000 }]);
  await expect(cards).toHaveCount(1);
  const deletedIcon = page.locator('.ol-notification-icon--deleted');
  await expect(deletedIcon).toBeVisible();
  const deletedColor = await deletedIcon.evaluate((node) => getComputedStyle(node).backgroundColor);
  const [deletedRed, deletedGreen] = deletedColor.match(/\d+/g)!.map(Number);
  expect(deletedRed).toBeGreaterThan(200);
  expect(deletedGreen).toBeLessThan(160);
});

test('save bridge and independent in-flight saves keep their own lifecycle', async ({ page }) => {
  await page.goto('/e2e/fixtures/notifications.html');
  await page.getByRole('button', { name: 'Start bridge' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Bridge' })).toBeVisible();
  await page.getByRole('button', { name: 'Idle bridge' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Bridge' })).toHaveCount(0);
  await page.getByRole('button', { name: 'Save bridge' }).click();
  await page.getByRole('button', { name: 'Undo' }).click();
  await expect(page.getByText('Action invoked')).toBeVisible();
  await page.getByRole('button', { name: 'Save bridge' }).click();
  await expect(page.getByRole('button', { name: 'Undo' })).toBeVisible();
  await page.getByRole('button', { name: 'Start first' }).click();
  await page.getByRole('button', { name: 'Start second' }).click();
  await page.getByRole('button', { name: 'Finish first' }).click();
  await expect(page.getByText('First saving')).toHaveCount(0);
  await expect(page.getByText('First saved')).toBeVisible();
  await expect(page.getByText('Second saving')).toBeVisible();
});

for (const kind of ['qa', 'voice-edit', 'selection-voice-intent']) {
  test(`auxiliary ${kind} window has its own notification host`, async ({ page }) => {
    await page.goto(`/?window=${kind}&os=mac`);
    await expect(page.locator('.ol-notification-center')).toHaveCount(1);
    await emitNotifications(page, [{ state: 'saved', message: 'QA notification' }]);
    await expect(page.getByRole('status').filter({ hasText: 'QA notification' })).toBeVisible();
  });
}

test('progress notification paints above a portaled channel editor', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await page.locator('.ol-channel-edit-button').first().click();
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();

  await emitNotifications(page, [
    { state: 'saving', message: '正在处理…', progress: true, durationMs: 0 },
  ]);
  const card = page.locator('.ol-notification-card');
  await expect(card).toBeVisible();
  await expect(page.locator('.ol-notification-progress')).toBeVisible();
  // The drop-in spring starts above the window; measure after it settles.
  await page.waitForTimeout(500);
  const box = (await card.boundingBox())!;
  expect(
    await page.evaluate(
      ([x, y]) => !!document.elementFromPoint(x, y)?.closest('.ol-notification-card'),
      [box.x + box.width / 2, box.y + box.height / 2],
    ),
  ).toBe(true);
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();
});
