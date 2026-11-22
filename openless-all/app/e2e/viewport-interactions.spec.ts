import { fixtureOrigin } from './support/server';
import { expect, test, type Locator } from '@playwright/test';

const fixture = `${fixtureOrigin}/e2e/fixtures/viewport-interactions.html`;

async function bounds(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error('No painted box for the requested element.');
  return box;
}

test.use({ reducedMotion: 'reduce' });

test('replacement, Escape, scope disposal, and dialog choices settle only their own request', async ({
  page,
}) => {
  await page.goto(fixture);
  const trigger = page.getByRole('button', { name: 'First confirmation' });
  await trigger.click();
  const first = page.getByRole('dialog', { name: 'First prompt' });
  await expect(first.getByRole('button', { name: 'Keep unchanged' })).toBeFocused();
  await page.evaluate(() => (window as { askAgain?: () => void }).askAgain?.());
  await expect(page.getByRole('dialog', { name: 'New prompt' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByText('First prompt:false')).toHaveCount(1);
  await expect(page.getByText('New prompt:false')).toHaveCount(1);
  await expect(trigger).toBeFocused();

  await page.getByRole('button', { name: 'Request from scope' }).click();
  await expect(page.getByRole('dialog', { name: 'Scoped request' })).toBeVisible();
  await page.evaluate(() => (window as { disposePage?: () => void }).disposePage?.());
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByText('scope:false')).toHaveCount(1);

  await page.getByRole('button', { name: 'Custom dialog' }).click();
  await expect(page.getByRole('dialog', { name: 'Pick a result' })).toBeVisible();
  const old = await page.evaluateHandle(
    () => (window as { dialogResolve?: (value: string) => void }).dialogResolve,
  );
  await page.evaluate(() => (window as { askAgain?: () => void }).askAgain?.());
  await old.evaluate((resolve) => resolve?.('stale'));
  const newPrompt = page.getByRole('dialog', { name: 'New prompt' });
  await expect(newPrompt).toBeVisible();
  await newPrompt.getByRole('button', { name: 'Proceed with change' }).click();
  await expect(page.getByText('dialog:undefined')).toHaveCount(1);
  await expect(page.getByText('dialog:stale')).toHaveCount(0);
  await expect(page.getByText('New prompt:true')).toHaveCount(1);

  await page.getByRole('button', { name: 'Custom dialog' }).click();
  await page.getByRole('button', { name: 'Choose result' }).click();
  await expect(page.getByText('dialog:chosen')).toHaveCount(1);
});

test('the top layer owns pointer, keyboard, and focus; a nested combobox closes before the modal', async ({
  page,
}) => {
  await page.goto(fixture + '?frame');
  const trigger = page.getByRole('button', { name: 'Parent modal' });
  await trigger.click();
  const parent = page.getByRole('dialog', { name: 'Parent dialog' });
  await expect(parent).toBeFocused();
  await page.keyboard.press('Tab');
  const nestedTrigger = parent.getByRole('button', { name: 'Nested confirmation' });
  await expect(nestedTrigger).toBeFocused();
  await nestedTrigger.click();
  const confirm = page.getByRole('dialog', { name: 'Nested prompt' });
  await expect(confirm).toBeVisible();
  await expect(parent).toHaveCount(0);
  const cancel = confirm.getByRole('button', { name: 'Keep unchanged' });
  const proceed = confirm.getByRole('button', { name: 'Proceed with change' });
  const close = confirm.getByRole('button', { name: '关闭', exact: true });
  await expect(cancel).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(close).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(proceed).toBeFocused();
  await page.keyboard.press('Tab');
  await expect(close).toBeFocused();

  const viewport = await bounds(page.locator('.ol-app-viewport'));
  const top = await bounds(page.locator('.ol-dialog-overlay:not([inert])'));
  expect(top).toEqual(viewport);
  await page.mouse.click(viewport.x / 2, viewport.y + 1);
  await expect(confirm).toBeVisible();
  await page.mouse.click(viewport.x + 2, viewport.y + 2);
  await expect(confirm).toHaveCount(0);
  await expect(parent).toBeVisible();
  await expect(nestedTrigger).toBeFocused();

  const picker = parent.getByRole('combobox', { name: 'Searchable choices' });
  await picker.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.mouse.click(viewport.x + 2, viewport.y + 2);
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await expect(parent).toBeVisible();

  await picker.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await expect(parent).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(parent).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

for (const [scale, factor] of [
  ['small', 0.9],
  ['medium', 1],
  ['large', 1.1],
] as const) {
  test(`viewports and collision use real coordinates at ${Math.round(factor * 100)}%`, async ({
    page,
  }) => {
    await page.goto(
      fixture + `?frame&scale=${scale}&theme=${scale === 'large' ? 'dark' : 'light'}`,
    );
    const viewport = page.locator('.ol-app-viewport');
    const content = page.locator('.ol-viewport-content');
    const initial = await bounds(viewport);
    expect(initial.width).toBe(700);
    expect(initial.height).toBe(460);
    expect(await bounds(content)).toEqual(initial);
    const picker = page.getByRole('combobox', { name: 'Window options' });
    await picker.click();
    const list = page.getByRole('listbox');
    await expect(list).toBeVisible();
    const positioner = page.locator('.ol-select-positioner');
    await expect(positioner).toHaveAttribute('data-side', 'top');
    const popup = await bounds(positioner);
    const anchor = await bounds(picker);
    expect(Math.abs(popup.width - anchor.width)).toBeLessThan(2);
    expect(popup.y).toBeGreaterThanOrEqual(initial.y);
    expect(popup.y + popup.height).toBeLessThan(anchor.y);
    expect(popup.x + popup.width).toBeLessThanOrEqual(initial.x + initial.width);
    await page.getByRole('option', { name: 'Choice 48', exact: true }).scrollIntoViewIfNeeded();
    await page.getByRole('option', { name: 'Choice 48', exact: true }).click();
    await expect(picker).toContainText('Choice 48');

    await page.getByRole('button', { name: 'First confirmation' }).click();
    const confirm = page.getByRole('dialog', { name: 'First prompt' });
    expect((await bounds(confirm)).width / factor).toBeCloseTo(400, 0);
    await page.keyboard.press('Escape');

    await page.setViewportSize({ width: 330, height: 356 });
    await page.goto(fixture + `?scale=${scale}`);
    await page.getByRole('button', { name: 'Long confirmation' }).click();
    const long = page.getByRole('dialog', { name: 'Long prompt' });
    const card = await bounds(long);
    expect(card.x).toBeGreaterThanOrEqual(0);
    expect(card.x + card.width).toBeLessThanOrEqual(330);
    expect(card.y).toBeGreaterThanOrEqual(0);
    expect(card.y + card.height).toBeLessThanOrEqual(356);
    const body = long.locator('.ol-dialog-body');
    const lastLine = long.getByTestId('long-detail-end');
    const cancel = long.getByRole('button', { name: 'Keep unchanged' });
    await expect.poll(() => body.evaluate((node) => node.scrollHeight - node.clientHeight))
      .toBeGreaterThan(0);
    await expect(lastLine).not.toBeInViewport();
    await expect(cancel).toBeInViewport();
    const footerBefore = await bounds(cancel);
    await body.hover();
    await page.mouse.wheel(0, 20_000);
    await expect(lastLine).toBeInViewport();
    const footerAfter = await bounds(cancel);
    expect(Math.abs(footerAfter.y - footerBefore.y)).toBeLessThan(1);
    expect(await long.evaluate((node) => node.scrollTop)).toBe(0);
    await cancel.click();
    await expect(page.getByText('Long prompt:false')).toBeVisible();
  });
}

test('the heatmap keeps the 53 columns and its expandable list inside its own scroll areas', async ({
  page,
}) => {
  await page.goto(fixture + '?activity&scale=large');
  const card = page.locator('.ol-gha');
  expect((await bounds(card)).width / 1.1).toBeCloseTo(771, 0);
  const cells = card.locator('.ol-gha-cell');
  await expect(cells).toHaveCount(365);
  const first = await bounds(cells.nth(0));
  const second = await bounds(cells.nth(1));
  expect(first.width / 1.1).toBeCloseTo(11, 0);
  expect((second.y - first.y - first.height) / 1.1).toBeCloseTo(3, 0);
  await cells.nth(0).hover();
  await expect(page.getByRole('tooltip')).toContainText('2025-01-01');

  await page.setViewportSize({ width: 340, height: 500 });
  const grid = card.locator('.ol-gha-grid');
  await expect
    .poll(() => grid.evaluate((node) => node.scrollWidth - node.clientWidth))
    .toBeGreaterThan(100);
  await expect(cells).toHaveCount(365);
  await grid.evaluate((node) => {
    node.scrollLeft = node.scrollWidth;
  });
  await cells.last().hover();
  await expect(page.getByRole('tooltip')).toContainText('2025-12-31');
  const expand = card.getByRole('button', { name: 'Show top apps' });
  await expand.click();
  const apps = card.getByRole('list');
  await expect(apps).toBeVisible();
  await expect
    .poll(() => apps.evaluate((node) => node.scrollHeight - node.clientHeight))
    .toBeGreaterThan(100);
  await apps.getByText('Application 35', { exact: true }).scrollIntoViewIfNeeded();
  await expect(apps.getByText('Application 35', { exact: true })).toBeInViewport();
  await card.getByRole('button', { name: 'Hide top apps' }).click();
  await expect(apps).toHaveCount(0);
});

test('warnings wait for acknowledgment and success notifications only show status', async ({
  page,
}) => {
  await page.goto(fixture + '?theme=dark');
  await page.getByRole('button', { name: 'Warning' }).click();
  const warning = page.getByRole('dialog', { name: 'Needs attention' });
  await expect(warning).toBeVisible();
  await expect(page.getByText('warning:true')).toHaveCount(0);
  await warning.getByRole('button', { name: 'Acknowledge' }).click();
  await expect(page.getByText('warning:true')).toHaveCount(1);
  await page.getByRole('button', { name: 'Show feedback' }).click();
  await expect(page.getByRole('status')).toHaveText('Status only');
  const icon = page.locator('.ol-notification-icon');
  await expect(icon).toHaveCSS('background-color', 'rgb(59, 130, 246)');
  await page.locator('.ol-notification-close').click();
  await expect(page.getByRole('status')).toHaveCount(0);
});
