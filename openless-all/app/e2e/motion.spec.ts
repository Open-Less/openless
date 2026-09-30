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

test('settings entrance paints its starting pose and progresses over time', async ({ page }) => {
  await page.evaluate(() => {
    const probe = window as Window & {
      settingsEntranceFrames: { opacity: number; y: number; scale: number }[];
      settingsEntranceComplete: boolean;
    };
    probe.settingsEntranceFrames = [];
    probe.settingsEntranceComplete = false;
    const observer = new MutationObserver(() => {
      const panel = document.querySelector<HTMLElement>('.ol-settings-surface');
      if (!panel) return;
      observer.disconnect();
      const deadline = performance.now() + 15_000;
      const sample = () => {
        const style = getComputedStyle(panel);
        const transform = new DOMMatrixReadOnly(style.transform);
        probe.settingsEntranceFrames.push({
          opacity: Number(style.opacity),
          y: transform.m42,
          scale: transform.m11,
        });
        if (
          Number(style.opacity) === 1 &&
          !panel.getAnimations().some((animation) => animation.id === 'ol-surface-enter')
        ) {
          probe.settingsEntranceComplete = true;
          return;
        }
        // Sample through the delayed start and completion without requiring a fixed frame rate.
        if (performance.now() < deadline) window.setTimeout(sample, 16);
      };
      requestAnimationFrame(sample);
    });
    observer.observe(document.body, { subtree: true, childList: true });
  });
  await page.getByRole('button', { name: '设置', exact: true }).press('Enter');
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect
    .poll(
      () =>
        page.evaluate(
          () => (window as Window & { settingsEntranceComplete: boolean }).settingsEntranceComplete,
        ),
      { timeout: 15_000 },
    )
    .toBe(true);
  const frames = await page.evaluate(
    () =>
      (
        window as Window & {
          settingsEntranceFrames: { opacity: number; y: number; scale: number }[];
        }
      ).settingsEntranceFrames,
  );
  expect(frames[0].opacity).toBeLessThan(0.1);
  expect(frames[0].y).toBeGreaterThan(16);
  expect(frames[0].scale).toBeLessThan(0.97);
  expect(frames.some((frame) => frame.opacity > 0.1 && frame.opacity < 0.9)).toBe(true);
  expect(new Set(frames.map((frame) => frame.opacity)).size).toBeGreaterThan(3);
  await expect(dialog).toHaveCSS('opacity', '1');
  await expect(dialog).toHaveCSS('transform', 'none');
  await expect
    .poll(() => dialog.evaluate((element) => (element as HTMLElement).style.willChange))
    .toBe('');
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
      if (panel && animation && !panel.dataset.testEntryPending) {
        panel.dataset.testEntryPending = 'true';
        // Pin after the dialog's first-frame startup callback has resumed the entrance.
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            animation.pause();
            animation.currentTime = 72;
            panel.dataset.testEntryOpacity = getComputedStyle(panel).opacity;
          });
        });
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
        entries: navigation.querySelectorAll(':scope > button').length,
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

test('selection highlights move and settle correctly after a rapid mode return', async ({
  page,
}) => {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (...args) {
      const animation = animate.apply(this, args);
      queueMicrotask(() => {
        if (animation.id === 'ol-selection-move') {
          const frames = (animation.effect as KeyframeEffect).getKeyframes();
          if (frames[0].transform !== frames.at(-1)!.transform) {
            document.body.dataset.selectionMoved = 'true';
          }
          const indicator = this as HTMLElement;
          if (!indicator.dataset.testSelectionPose) {
            animation.pause();
            animation.currentTime = 72;
            indicator.dataset.testSelectionPose = getComputedStyle(indicator).transform;
          } else {
            indicator.dataset.testSelectionFrom = String(frames[0].transform);
          }
        }
      });
      return animation;
    };
  });
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).press('Enter');
  await expect(page.locator('body')).toHaveAttribute('data-selection-moved', 'true');
  await dialog.getByRole('button', { name: '传统模式', exact: true }).press('Enter');
  for (const selector of ['.ol-service-pipeline-indicator', '.ol-service-view-indicator']) {
    const indicator = dialog.locator(selector);
    await expect(indicator).toHaveAttribute('data-test-selection-from', /.+/);
    expect(await indicator.getAttribute('data-test-selection-from')).toBe(
      await indicator.getAttribute('data-test-selection-pose'),
    );
    await expect
      .poll(() =>
        dialog.locator(selector).evaluate((indicator) => {
          const selected = indicator.parentElement!.querySelector<HTMLElement>(
            'button[aria-pressed="true"]',
          )!;
          return Math.abs(
            indicator.getBoundingClientRect().left - selected.getBoundingClientRect().left,
          );
        }),
      )
      .toBeLessThan(1);
    await expect
      .poll(() =>
        dialog.locator(selector).evaluate((element) => (element as HTMLElement).style.willChange),
      )
      .toBe('');
  }
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).click();
  expect(
    await dialog.evaluate(
      (element) =>
        element
          .getAnimations({ subtree: true })
          .filter((animation) => animation.id === 'ol-selection-move').length,
    ),
  ).toBe(0);
});

test('provider changes reveal configuration and validation feedback', async ({ page }) => {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await dialog.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
  await dialog.getByRole('button', { name: '多模态模式', exact: true }).click();
  const form = dialog.locator('.ol-omni-settings > div');
  // Retain the actual entrance at an intermediate frame for deterministic inspection.
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    Element.prototype.animate = function (...args) {
      const animation = animate.apply(this, args);
      if (this.matches('.ol-omni-settings > div')) {
        animation.pause();
        animation.currentTime = 72;
      }
      return animation;
    };
  });
  await dialog.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: '阿里云百炼 Omni', exact: true }).click();
  await expect(dialog.getByLabel('额外 Headers', { exact: true })).toHaveCount(0);
  const opacity = Number(await form.evaluate((element) => getComputedStyle(element).opacity));
  expect(opacity).toBeGreaterThan(0);
  expect(opacity).toBeLessThan(1);
  await form.evaluate((element) =>
    element.getAnimations().forEach((animation) => animation.play()),
  );
  await expect
    .poll(() => form.evaluate((element) => (element as HTMLElement).style.willChange))
    .toBe('');
  await dialog.getByRole('button', { name: '验证', exact: true }).click();
  await expect(dialog.locator('.ol-provider-result')).toHaveAttribute('data-status', 'success');
  await expect(dialog.locator('.ol-provider-result')).toHaveCSS('animation-name', 'ol-feedback-in');
  await expect(dialog.getByRole('button', { name: '验证', exact: true })).toBeEnabled();
});
