import { expect, test, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';
import type { NotificationOptions } from '../src/api/interaction';

test.use({ baseURL: fixtureOrigin });

/** Import the exact module instance the app loaded (Vite appends an HMR query). */
async function emitNotifications(
  page: Page,
  items: (Omit<NotificationOptions, 'onAction' | 'actionOwner'> & { id?: string })[],
) {
  await page.evaluate(async (items) => {
    const url = performance
      .getEntriesByType('resource')
      .map((entry) => entry.name)
      .find((name) => name.includes('/src/lib/notifications.ts'));
    const { notify, dismissNotification } = await import(url || '/src/lib/notifications.ts');
    for (const item of items) {
      const { id, actionLabel } = item;
      notify({ ...item, onAction: id && actionLabel ? () => dismissNotification(id) : undefined });
    }
  }, items);
}

test('notifications stack above settings, preserve errors and support dismissal', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
  await page.goto('/?os=mac');
  await expect(page.locator('.ol-notification-center')).toHaveCount(1);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await emitNotifications(page, [
    { state: 'saved', message: 'First notification', durationMs: 10000 },
    { state: 'failed', message: 'Second notification' },
  ]);
  const cards = page.locator('.ol-notification-card');
  await expect(cards).toHaveCount(2);
  await expect(page.getByRole('alert').filter({ hasText: 'Second notification' })).toBeVisible();
  const center = await cards.first().boundingBox();
  const viewport = (await page.locator('.ol-app-viewport').boundingBox())!;
  expect(Math.abs(center!.x + center!.width / 2 - viewport.x - viewport.width / 2)).toBeLessThan(2);
  await cards.last().getByRole('button').click();
  await expect(cards).toHaveCount(1);
});

test('a new save replaces the previous receipt in place and draws a full success check', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'large'));
  await page.goto('/?os=mac');
  await expect(page.locator('.ol-notification-center')).toHaveCount(1);
  await emitNotifications(page, [
    { state: 'saved', message: '同步设置已更新。', durationMs: 10000 },
  ]);
  const oldCard = page.locator('.ol-notification-card').filter({ hasText: '同步设置已更新。' });
  const replacement = page
    .locator('.ol-notification-card')
    .filter({ hasText: '正在保存 DeepSeek 配置' });
  await expect(oldCard).toHaveCSS('transform', 'none');
  const anchor = (await oldCard.boundingBox())!;
  await oldCard.evaluate((node) => {
    const original = node.animate;
    node.animate = function (keyframes, options) {
      const animation = original.call(this, keyframes, options);
      animation.pause();
      animation.currentTime = 0;
      return animation;
    };
  });
  await emitNotifications(page, [
    { id: 'deepseek', state: 'saving', message: '正在保存 DeepSeek 配置' },
  ]);
  await expect(oldCard).toHaveAttribute('aria-hidden', 'true');
  await expect(replacement).toHaveCSS('transform', 'none');
  const start = (await oldCard.boundingBox())!;
  expect(start.y).toBeCloseTo(anchor.y, 0);
  expect(start.x).toBeCloseTo(anchor.x, 0);
  expect(start.width).toBeCloseTo(anchor.width, 0);
  expect(start.height).toBeCloseTo(anchor.height, 0);
  expect((await replacement.boundingBox())!.y).toBeCloseTo(anchor.y, 0);
  const halfway = await oldCard.evaluate((node) => {
    const exit = node.getAnimations().find((animation) => animation.id === 'ol-notice-exit')!;
    exit.currentTime = Number(exit.effect!.getTiming().duration) / 2;
    const box = node.getBoundingClientRect();
    const pose = {
      x: box.x,
      y: box.y,
      width: box.width,
      height: box.height,
      opacity: Number(getComputedStyle(node).opacity),
    };
    exit.play();
    return pose;
  });
  expect(halfway.y).toBeLessThan(anchor.y);
  expect(halfway.x).toBeCloseTo(anchor.x, 0);
  expect(halfway.width).toBeCloseTo(anchor.width, 0);
  expect(halfway.height).toBeCloseTo(anchor.height, 0);
  expect(halfway.opacity).toBeGreaterThan(0);
  expect(halfway.opacity).toBeLessThan(1);
  const cards = page.locator('.ol-notification-card');
  await expect(cards).toHaveCount(1);
  await emitNotifications(page, [
    { id: 'deepseek', state: 'saved', message: 'DeepSeek 配置已保存', durationMs: 10000 },
  ]);
  await expect(cards.first()).toContainText('DeepSeek 配置已保存');
  const message = cards.locator('.ol-notification-message');
  expect(await message.evaluate((node) => node.clientHeight)).toBe(20);
  const box = (await cards.first().boundingBox())!;
  const bounds = (await page.locator('.ol-app-viewport').boundingBox())!;
  expect(box.y - bounds.y).toBeLessThan(12);
  expect(
    await cards.first().evaluate((node: HTMLElement) => [node.offsetWidth, node.offsetHeight]),
  ).toEqual([356, 56]);
  const savedIcon = cards.locator('[data-slot="toast-icon"]');
  await expect(savedIcon).toHaveAttribute('data-tone', 'success');
  expect(await savedIcon.evaluate((node: HTMLElement) => node.offsetWidth)).toBe(36);

  const stroke = await savedIcon.locator('.ol-toast-stroke').evaluate((path: SVGPathElement) => {
    const draw = path.getAnimations()[0];
    const timing = draw.effect!.getTiming();
    draw.pause();
    draw.currentTime = timing.delay;
    const initialOffset = parseFloat(getComputedStyle(path).strokeDashoffset);
    draw.currentTime = timing.delay + Number(timing.duration) / 2;
    const offset = parseFloat(getComputedStyle(path).strokeDashoffset);
    draw.finish();
    return {
      initialOffset,
      offset,
      length: path.getTotalLength(),
      dash: parseFloat(getComputedStyle(path).strokeDasharray),
      endOffset: parseFloat(getComputedStyle(path).strokeDashoffset),
      normalized: path.hasAttribute('pathLength'),
    };
  });
  expect(stroke.normalized).toBe(false);
  expect(stroke.dash).toBeGreaterThan(stroke.length);
  expect(stroke.initialOffset).toBeGreaterThan(stroke.length);
  expect(stroke.offset).toBeGreaterThan(0);
  expect(stroke.offset).toBeLessThan(stroke.length);
  expect(stroke.endOffset).toBe(0);

  await emitNotifications(page, [{ state: 'deleted', message: '已删除', durationMs: 10000 }]);
  await expect(cards).toHaveCount(1);
  await expect(message).toHaveText('已删除');
  expect(await message.evaluate((node) => node.clientHeight)).toBe(20);
  await expect(savedIcon).toHaveAttribute('data-tone', 'trash');
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
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'medium'));
  await page.goto('/?os=mac');
  await expect(page.locator('.ol-notification-center')).toHaveCount(1);
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
  await page.locator('.ol-channel-edit-button').first().click();
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();

  await emitNotifications(page, [
    { state: 'saving', message: '正在处理…', progress: true, durationMs: 0 },
  ]);
  const card = page.locator('.ol-notification-card');
  await expect(card).toBeVisible();
  await expect(page.locator('.ol-notification-progress')).toBeVisible();
  await expect
    .poll(() =>
      card.evaluate((node) => {
        const box = node.getBoundingClientRect();
        return !!document
          .elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)
          ?.closest('.ol-notification-card');
      }),
    )
    .toBe(true);
  await expect(page.locator('.ol-channel-dialog')).toBeVisible();
});

test('compact progress centers its track below status and fades the departing cancel action', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/notifications.html');
  await expect(page.locator('.ol-notification-center')).toHaveCount(1);
  await emitNotifications(page, [
    {
      id: 'download',
      state: 'saving',
      message: 'Downloading…',
      progress: true,
      progressValue: 42,
      actionLabel: 'Cancel download',
      actionStyle: 'cancel-icon',
    },
  ]);
  const card = page.locator('.ol-notification-card');
  const progress = card.getByRole('progressbar', { name: 'Downloading…' });
  const status = card.getByRole('status');
  const cancel = card.getByRole('button', { name: 'Cancel download', exact: true });
  await expect(card).toHaveCSS('transform', 'none');
  await expect(progress).toHaveAttribute('aria-valuenow', '42');
  await expect(progress).toHaveCSS('width', '184px');
  await expect(status.locator('.ol-notification-message')).toHaveCSS('font-size', '12px');
  await expect(cancel).toHaveAttribute('data-action-style', 'cancel-icon');
  const [track, text, action] = await Promise.all([
    progress.boundingBox(),
    status.boundingBox(),
    cancel.boundingBox(),
  ]);
  expect(text!.y + text!.height).toBeLessThan(track!.y);
  expect(track!.x + track!.width / 2).toBeCloseTo(text!.x + text!.width / 2, 0);
  expect(action!.x).toBeGreaterThan(track!.x + track!.width);
  expect(await card.evaluate((node) => node.clientHeight)).toBe(56);

  const retiring = await cancel.evaluateHandle((button: HTMLButtonElement) => {
    const frames: { disabled: boolean; opacity: number }[] = [];
    const observer = new MutationObserver(() => {
      if (button.getAttribute('aria-hidden') !== 'true') return;
      observer.disconnect();
      function sample() {
        if (!button.isConnected) return;
        frames.push({
          disabled: button.disabled,
          opacity: Number(getComputedStyle(button).opacity),
        });
        requestAnimationFrame(sample);
      }
      requestAnimationFrame(sample);
    });
    observer.observe(button, { attributes: true, attributeFilter: ['aria-hidden'] });
    return frames;
  });
  await emitNotifications(page, [{ id: 'download', state: 'saved', message: 'Download complete' }]);
  await expect(status).toHaveText('Download complete');
  await expect(card.locator('[data-slot="toast-action"]')).toHaveCount(0);
  const frames = await retiring.jsonValue();
  expect(frames.some(({ opacity }) => opacity > 0 && opacity < 1)).toBe(true);
  expect(frames.every(({ disabled }) => disabled)).toBe(true);
});

for (const scale of ['small', 'medium', 'large']) {
  test(`status notice enters and exits completely beyond the content top at ${scale}`, async ({
    page,
  }) => {
    await page.addInitScript(() => {
      const original = Element.prototype.animate;
      const samples: { phase: string; start: number; top: number; end?: number }[] = [];
      Object.assign(window, { noticeMotionSamples: samples });
      Element.prototype.animate = function (keyframes, options) {
        const animation = original.call(this, keyframes, options);
        if (!this.classList.contains('ol-notification-card')) return animation;
        const frames = Array.isArray(keyframes)
          ? keyframes
          : (animation.effect as KeyframeEffect).getKeyframes();
        const sample = {
          phase: frames[frames.length - 1].transform === 'none' ? 'enter' : 'exit',
          start: 0,
          top: this.closest('.ol-app-viewport')!.getBoundingClientRect().top,
        } as { phase: string; start: number; top: number; end?: number };
        animation.pause();
        animation.currentTime = 0;
        sample.start = this.getBoundingClientRect().bottom;
        samples.push(sample);
        animation.play();
        // Register before the component's completion handler removes its retained exit frame.
        void animation.finished
          .then(() => {
            sample.end = this.getBoundingClientRect().bottom;
          })
          .catch(() => {});
        return animation;
      };
    });
    await page.goto(
      `${fixtureOrigin}/e2e/fixtures/viewport-interactions.html?frame&scale=${scale}&theme=dark`,
    );
    await page.getByRole('button', { name: 'Show feedback', exact: true }).click();
    const notice = page.locator('.ol-notification-card');
    await expect(notice).toHaveCSS('transform', 'none');
    await notice.getByRole('button').click();
    await expect(notice).toHaveCount(0);
    const samples = await page.evaluate(
      () =>
        (window as any).noticeMotionSamples as {
          phase: string;
          start: number;
          top: number;
          end?: number;
        }[],
    );
    const entries = samples.filter((sample) => sample.phase === 'enter');
    expect(entries.length).toBeGreaterThan(0);
    for (const entry of entries) expect(entry.start).toBeLessThanOrEqual(entry.top);
    const exit = [...samples].reverse().find((sample) => sample.phase === 'exit');
    expect(exit?.end).toBeDefined();
    expect(exit!.end!).toBeLessThanOrEqual(exit!.top);
  });
}

test('long severe notices wrap in a short narrow window and reduced motion dismisses immediately', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 330, height: 200 });
  await page.goto(`${fixtureOrigin}/e2e/fixtures/notifications.html`);
  const message =
    '删除提醒：请在继续之前确认云端状态。长文本必须完整显示，关闭按钮仍可操作。'.repeat(3);
  await emitNotifications(page, [{ state: 'info', severity: 'warning', message }]);
  const notice = page.locator('.ol-notification-card');
  await expect(notice).toHaveAttribute('data-variant', 'alert');
  await expect(notice).toHaveAttribute('data-severity', 'warning');
  await expect(notice.getByRole('alert')).toHaveText(message);
  await expect(notice.locator('[data-slot="toast-icon"]')).toHaveCSS(
    'background-color',
    'rgb(255, 204, 0)',
  );
  expect(await notice.evaluate((node) => node.scrollWidth <= node.clientWidth + 1)).toBe(true);
  await expect(notice).toHaveCSS('transform', 'none');
  await notice.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(notice).toHaveCount(0);
});

test('a notice finishes outside the viewport after reflow during exit', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/notifications.html`);
  await emitNotifications(page, [
    {
      state: 'info',
      message:
        '正在同步云端最新内容，请等待当前任务完成。长文案在窗口改变时应继续完整换行。'.repeat(2),
      durationMs: 0,
    },
  ]);
  const notice = page.locator('.ol-notification-card');
  await expect(notice).toHaveCSS('transform', 'none');
  await page.evaluate(() => {
    const original = Element.prototype.animate;
    const motion = {
      latest: undefined as Animation | undefined,
      attempts: [] as Animation[],
      finished: [] as { bottom: number; top: number; connected: boolean }[],
    };
    Object.assign(window, { noticeResizeMotion: motion });
    Element.prototype.animate = function (keyframes, options) {
      const animation = original.call(this, keyframes, options);
      if (!this.classList.contains('ol-notification-card')) return animation;
      motion.latest = animation;
      motion.attempts.push(animation);
      // Hold each real exit halfway through so the layout changes before it finishes.
      animation.pause();
      animation.currentTime = Number(animation.effect!.getTiming().duration) / 2;
      void animation.finished
        .then(() => {
          motion.finished.push({
            bottom: this.getBoundingClientRect().bottom,
            top: this.closest('.ol-app-viewport')!.getBoundingClientRect().top,
            connected: this.isConnected,
          });
        })
        .catch(() => {});
      return animation;
    };
  });
  await notice.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(notice).toHaveAttribute('aria-hidden', 'true');
  expect(await notice.evaluate((node) => node.getBoundingClientRect().bottom)).toBeGreaterThan(0);
  await notice.evaluate(async (node) => {
    const { applyFontScale } = await import('/src/lib/fontScale.ts');
    applyFontScale('large');
    node.closest<HTMLElement>('.ol-app-viewport')!.style.width = '290px';
    node.querySelector('.ol-notification-message')!.textContent +=
      '更长的说明使退出中的卡片重新排版。'.repeat(3);
  });
  await expect
    .poll(() => page.evaluate(() => (window as any).noticeResizeMotion.attempts.length))
    .toBeGreaterThanOrEqual(2);
  await expect(notice).toHaveCount(1);
  const resized = await notice.evaluate((node) => {
    const viewport = node.closest('.ol-app-viewport')!.getBoundingClientRect();
    const card = node.getBoundingClientRect();
    return {
      fits: card.width < viewport.width,
      center: card.x + card.width / 2 - (viewport.x + viewport.width / 2),
    };
  });
  expect(resized.fits).toBe(true);
  expect(resized.center).toBeCloseTo(0, 0);
  await page.evaluate(() => (window as any).noticeResizeMotion.latest.play());
  await expect(notice).toHaveCount(0);
  const finished = await page.evaluate(
    () =>
      (window as any).noticeResizeMotion.finished as {
        bottom: number;
        top: number;
        connected: boolean;
      }[],
  );
  expect(finished).toHaveLength(1);
  expect(finished[0].connected).toBe(true);
  expect(finished[0].bottom).toBeLessThanOrEqual(finished[0].top);
  expect(
    await page.evaluate(() =>
      (window as any).noticeResizeMotion.attempts.every(
        (animation: Animation) => animation.playState === 'idle' && animation.onfinish === null,
      ),
    ),
  ).toBe(true);
});
