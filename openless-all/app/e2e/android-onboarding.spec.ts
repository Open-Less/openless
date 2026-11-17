import { expect, test, type Locator, type Page } from '@playwright/test';

const fixtureUrl = 'http://127.0.0.1:5326/e2e/fixtures/android-onboarding.html?os=android';
const surface = (page: Page) => page.locator('.ol-onboarding-layer > div');
const next = (page: Page) => page.getByRole('button', { name: '下一步', exact: true });
const back = (page: Page) => page.getByRole('button', { name: '上一步', exact: true });
const defer = (page: Page) => page.getByRole('button', { name: '先进入应用', exact: true });

async function reachOverlayConfig(page: Page) {
  await page.goto(fixtureUrl);
  for (let step = 1; step <= 3; step++) {
    await expect(page.getByText(`第 ${step} / 6 项`, { exact: true })).toBeVisible();
    await next(page).click();
  }
  await expect(page.getByText('悬浮窗配置', { exact: true })).toBeVisible();
  // These are the production AndroidPermissionsPanel controls, not filler content.
  await expect(page.getByRole('combobox')).toHaveCount(7);
  await expect(page.getByRole('slider')).toHaveValue('72');
  await surface(page).evaluate((element) => { element.scrollTop = 0; });
}

async function expectBoundedOverflow(page: Page) {
  const metrics = await surface(page).evaluate((element) => ({
    height: element.clientHeight,
    scrollHeight: element.scrollHeight,
    top: element.getBoundingClientRect().top,
    contentTop: element.firstElementChild!.getBoundingClientRect().top,
  }));
  expect(metrics.height).toBeGreaterThan(0);
  expect(metrics.height).toBeLessThanOrEqual(page.viewportSize()!.height);
  expect(metrics.scrollHeight).toBeGreaterThan(metrics.height);
  // A height-only fix with align-items:center loses the start of a tall card.
  expect(metrics.contentTop).toBeGreaterThanOrEqual(metrics.top);
  await expect(page.getByText('悬浮窗配置', { exact: true })).toBeInViewport();
}

async function fullyInViewport(target: Locator, page: Page) {
  const bounds = await target.boundingBox();
  return !!bounds && bounds.y >= 0 && bounds.y + bounds.height <= page.viewportSize()!.height;
}

async function wheelTo(page: Page, target: Locator) {
  // Do not use scrollIntoViewIfNeeded/click to manufacture navigation reachability.
  await page.mouse.move(24, page.viewportSize()!.height / 2);
  for (let attempt = 0; attempt < 15 && !(await fullyInViewport(target, page)); attempt++) {
    await page.mouse.wheel(0, 220);
    await page.waitForTimeout(80);
  }
  expect(await fullyInViewport(target, page)).toBe(true);
}

for (const viewport of [{ width: 390, height: 600 }, { width: 740, height: 360 }]) {
  test(`Android tall overlay card scrolls and retains navigation at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await reachOverlayConfig(page);
    await expectBoundedOverflow(page);
    await wheelTo(page, page.getByRole('slider'));
    await expect(page.getByRole('slider')).toBeInViewport();
    await wheelTo(page, defer(page));
    await expect(next(page)).toBeInViewport();
    await expect(back(page)).toBeInViewport();
    expect(await surface(page).evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
    await next(page).click();
    await expect(page.getByText('第 5 / 6 项', { exact: true })).toBeVisible();
    await back(page).click();
    await expect(page.getByText('悬浮窗配置', { exact: true })).toBeVisible();
    await wheelTo(page, defer(page));
    await back(page).click();
    await expect(page.getByText('第 3 / 6 项', { exact: true })).toBeVisible();
    await next(page).click();
    await wheelTo(page, defer(page));
    await defer(page).click();
    await expect(page.getByRole('heading', { name: 'Guide completed' })).toBeVisible();
    expect(errors).toEqual([]);
  });
}

test.describe('Android native touch scrolling', () => {
  test.use({ viewport: { width: 390, height: 600 }, hasTouch: true });

  test('swiping the production card reaches its last control and navigation', async ({ page, browserName }) => {
    test.skip(browserName !== 'chromium', 'Android WebView touch input is exercised through Chromium CDP');
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await reachOverlayConfig(page);
    await expectBoundedOverflow(page);
    const session = await page.context().newCDPSession(page);
    const swipe = async () => {
      // Real browser input, not synthetic DOM TouchEvents or a scrollTop assignment.
      // Stay on card padding, away from native selects and the size slider.
      await session.send('Input.dispatchTouchEvent', {
        type: 'touchStart', touchPoints: [{ x: 24, y: 520 }],
      });
      for (let y = 480; y >= 160; y -= 40) {
        await session.send('Input.dispatchTouchEvent', {
          type: 'touchMove', touchPoints: [{ x: 24, y }],
        });
        await page.waitForTimeout(20);
      }
      await session.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await page.waitForTimeout(100);
    };
    const swipeTo = async (target: Locator) => {
      for (let attempt = 0; attempt < 12 && !(await fullyInViewport(target, page)); attempt++) {
        await swipe();
      }
      expect(await fullyInViewport(target, page)).toBe(true);
    };
    try {
      await swipeTo(page.getByRole('slider'));
      expect(await surface(page).evaluate((element) => element.scrollTop)).toBeGreaterThan(0);
      await swipeTo(defer(page));
      await expect(next(page)).toBeInViewport();
      await expect(back(page)).toBeInViewport();
      await next(page).tap();
      await expect(page.getByText('第 5 / 6 项', { exact: true })).toBeVisible();
      await back(page).tap();
      await expect(page.getByText('悬浮窗配置', { exact: true })).toBeVisible();
      await swipeTo(defer(page));
      await defer(page).tap();
      await expect(page.getByRole('heading', { name: 'Guide completed' })).toBeVisible();
      expect(errors).toEqual([]);
    } finally {
      await session.detach();
    }
  });
});
