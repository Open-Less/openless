import { expect, test, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';
import { settleNavigation } from './support/navigation';

test.use({ baseURL: fixtureOrigin });

/** Import the exact module instance the app loaded (Vite appends an HMR query). */
async function emitNotifications(
  page: Page,
  items: {
    state: 'saving' | 'saved' | 'deleted' | 'failed' | 'info';
    severity?: 'warning' | 'error';
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

test('transient confirmations drop from the window top and replace each other', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('ol-font-scale', 'large'));
  await page.goto('/?os=mac');
  await expect(page.locator('.ol-notification-center')).toHaveCount(1);
  await emitNotifications(page, [
    { state: 'saved', message: '同步设置已更新。', durationMs: 10000 },
    { state: 'saved', message: 'DeepSeek 配置已保存', durationMs: 10000 },
  ]);
  const cards = page.locator('.ol-notification-card');
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toContainText('DeepSeek 配置已保存');
  const message = cards.locator('.ol-notification-message');
  expect(await message.evaluate((node) => node.clientHeight)).toBe(18);
  const box = (await cards.first().boundingBox())!;
  // The host is flush with the window edge; the card itself keeps only the safe-area inset.
  const bounds = (await page.locator('.ol-app-viewport').boundingBox())!;
  expect(box.y - bounds.y).toBeLessThan(12);

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
  await expect(message).toHaveText('已删除');
  expect(await message.evaluate((node) => node.clientHeight)).toBe(18);
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
      attempts: 0,
      finished: [] as { bottom: number; top: number; connected: boolean }[],
    };
    Object.assign(window, { noticeResizeMotion: motion });
    Element.prototype.animate = function (keyframes, options) {
      const animation = original.call(this, keyframes, options);
      if (!this.classList.contains('ol-notification-card')) return animation;
      motion.latest = animation;
      motion.attempts++;
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
    .poll(() => page.evaluate(() => (window as any).noticeResizeMotion.attempts))
    .toBeGreaterThanOrEqual(2);
  await expect(notice).toHaveCount(1);
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
});
