import { expect, test, type Page } from '@playwright/test';

const errors = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const pageErrors: string[] = [];
  errors.set(page, pageErrors);
  page.on('pageerror', (error) => pageErrors.push(error.message));
  await page.goto('/');
  await expect(page.getByRole('button', { name: '设置', exact: true })).toBeVisible();
});
test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

test('rapid return to the current page cannot leave it transparent or inert', async ({ page }) => {
  for (let attempt = 0; attempt < 3; attempt++) {
    await page.getByRole('button', { name: '历史', exact: true }).press('Enter');
    await page.getByRole('button', { name: '概览', exact: true }).press('Enter');
    // Inspect beyond the complete transition budget to catch stale completion callbacks.
    await page.waitForTimeout(400);
    const content = page.locator('main .ol-scroll-fade');
    await expect(page.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
    await expect(content).toHaveCSS('opacity', '1');
    expect(await content.evaluate((element) => (element as HTMLElement).inert)).toBe(false);
    // WebKit may deliver the animation completion after its last painted frame.
    await expect
      .poll(() => content.evaluate((element) => (element as HTMLElement).style.willChange))
      .toBe('');
  }
});

test('closing during entry preserves the painted opacity and releases the dialog', async ({
  page,
}) => {
  await page.evaluate(() => {
    const observer = new MutationObserver(() => {
      const panel = document.querySelector<HTMLElement>('.ol-settings-surface');
      const animation = panel?.getAnimations().find((entry) => entry.id === 'ol-surface-enter');
      if (panel && animation && !panel.dataset.testEntryOpacity) {
        animation.pause();
        animation.currentTime = 72;
        panel.dataset.testEntryOpacity = getComputedStyle(panel).opacity;
      }
      const exit = panel?.getAnimations().find((entry) => entry.id === 'ol-surface-exit');
      if (exit) {
        document.body.dataset.testExitOpacity = String(
          (exit.effect as KeyframeEffect).getKeyframes()[0].opacity,
        );
        observer.disconnect();
      }
    });
    observer.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['style'],
    });
  });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(dialog).toHaveAttribute('data-test-entry-opacity', /.+/);
  const before = Number(await dialog.getAttribute('data-test-entry-opacity'));
  expect(before).toBeGreaterThan(0);
  expect(before).toBeLessThan(1);
  await dialog.getByRole('button', { name: '关闭', exact: true }).press('Enter');
  await expect(page.locator('body')).toHaveAttribute('data-test-exit-opacity', /.+/);
  const from = Number(await page.locator('body').getAttribute('data-test-exit-opacity'));
  expect(Math.abs(from - before)).toBeLessThan(0.001);
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '设置', exact: true })).toBeEnabled();
});

test('reduced motion keeps model gates and removes the closing delay', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await expect(dialog.getByRole('button', { name: /^多模态模型/ })).toBeDisabled();
  await dialog.getByRole('button', { name: '本地模型', exact: true }).click();
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).click();
  for (const label of ['语言模型', '语音识别', '本地模型']) {
    await expect(dialog.getByRole('button', { name: new RegExp(`^${label}。`) })).toBeDisabled();
  }
  await expect(dialog.getByRole('button', { name: '多模态模型', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  expect(
    await dialog.evaluate(
      (element) =>
        element
          .getAnimations({ subtree: true })
          .filter((animation) => animation.id.startsWith('ol-surface')).length,
    ),
  ).toBe(0);
  await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  expect(await dialog.count()).toBe(0);
});

test('mobile drawers retain their exit and can be opened again', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  const more = page.getByRole('button', { name: '更多', exact: true });
  await more.click();
  const drawer = page.getByRole('dialog', { name: '更多', exact: true });
  await expect(drawer).toHaveCSS('opacity', '1');
  await page.evaluate(() => {
    const observer = new MutationObserver(() => {
      const drawer = document.querySelector('[role="dialog"][aria-label="更多"]');
      if (drawer?.getAnimations().some((animation) => animation.id === 'ol-surface-exit')) {
        document.body.dataset.testDrawerExit = 'true';
        observer.disconnect();
      }
    });
    observer.observe(document.body, {
      subtree: true,
      attributes: true,
      attributeFilter: ['style'],
    });
  });
  await drawer.getByRole('button', { name: '关闭', exact: true }).press('Enter');
  await expect(page.locator('body')).toHaveAttribute('data-test-drawer-exit', 'true');
  await expect(drawer).toHaveCount(0);
  await more.click();
  await expect(drawer).toHaveCSS('opacity', '1');
  const rect = await drawer.boundingBox();
  expect(rect).not.toBeNull();
  expect(rect!.x).toBeGreaterThanOrEqual(-1);
  expect(rect!.x + rect!.width).toBeLessThanOrEqual(376);
});

test('changing reduced motion stops the decorative WebGL loop', async ({ page }) => {
  const supported = await page.evaluate(() => {
    const context = document.createElement('canvas').getContext('webgl');
    context?.getExtension('WEBGL_lose_context')?.loseContext();
    return Boolean(context);
  });
  test.skip(!supported, 'This runner does not provide a WebGL context');
  await page.evaluate(() => {
    const probe = window as Window & { motionDraws: number };
    probe.motionDraws = 0;
    const draw = WebGLRenderingContext.prototype.drawArrays;
    WebGLRenderingContext.prototype.drawArrays = function (...args) {
      probe.motionDraws++;
      return draw.apply(this, args);
    };
  });
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as Window & { motionDraws: number }).motionDraws))
    .toBeGreaterThan(5);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  // Let the preference change and its static repaint finish before checking for idle work.
  await page.waitForTimeout(300);
  const before = await page.evaluate(
    () => (window as Window & { motionDraws: number }).motionDraws,
  );
  await page.waitForTimeout(400);
  expect(await page.evaluate(() => (window as Window & { motionDraws: number }).motionDraws)).toBe(
    before,
  );
});

test('multimodal settings fit one desktop page and keep inactive notices below labels', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).click();
  const card = dialog.locator('.ol-omni-settings');
  await expect(card.getByLabel('额外 Headers', { exact: true })).toBeVisible();
  for (const viewport of [
    { width: 1300, height: 835 },
    { width: 1280, height: 720 },
  ]) {
    await page.setViewportSize(viewport);
    const bounds = await card.evaluate((element) => {
      const scroll = element.closest('.ol-thinscroll')!;
      const navigation = document.querySelector('.ol-service-views')!;
      return {
        verticalOverflow: scroll.scrollHeight - scroll.clientHeight,
        horizontalOverflow: navigation.scrollWidth - navigation.clientWidth,
        entries: navigation.children.length,
        badgesBelowLabels: [...navigation.querySelectorAll('.ol-service-inactive-tag')].every(
          (badge) =>
            badge.getBoundingClientRect().top >=
            badge.previousElementSibling!.getBoundingClientRect().bottom,
        ),
      };
    });
    expect(bounds.verticalOverflow).toBeLessThanOrEqual(1);
    expect(bounds.horizontalOverflow).toBeLessThanOrEqual(1);
    expect(bounds.entries).toBe(5);
    expect(bounds.badgesBelowLabels).toBe(true);
    await expect(card.getByRole('button', { name: '验证', exact: true })).toBeInViewport({
      ratio: 1,
    });
  }
  await dialog.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: '阿里云百炼 Omni', exact: true }).click();
  await expect(card.getByLabel('额外 Headers', { exact: true })).toHaveCount(0);
  await expect(card.getByRole('button', { name: '验证', exact: true })).toBeInViewport({
    ratio: 1,
  });
});
