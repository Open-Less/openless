import { expect, test, type Page } from '@playwright/test';
import { openMicrophoneSettings, settleNavigation } from './support/navigation';

async function settings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(dialog).toHaveCSS('transform', 'none');
  return dialog;
}

type NavigationProbe = Window & { restoreNavigationClock?: () => void };

test('a slow lazy page shows its own loading surface without keeping Overview on screen', async ({
  page,
}) => {
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(/\/assets\/Vocab-[^/]+\.js/, async (route) => {
    await gate;
    await route.continue();
  });
  await page.goto('/?os=mac');
  try {
    await page.getByRole('button', { name: '词典', exact: true }).click();
    const vocabulary = page.locator('main [data-ol-page="vocab"]');
    await expect(vocabulary).toHaveAttribute('data-transition-phase', 'settled');
    await expect(vocabulary).toHaveCSS('opacity', '1');
    await expect(vocabulary.locator('.ol-page-skeleton')).toBeVisible();
    await expect(page.locator('main [data-ol-page="overview"]')).toHaveCount(0);
    release();
    await expect(vocabulary.getByRole('heading', { name: '词典', exact: true })).toBeVisible();
    await expect(vocabulary.locator('.ol-page-skeleton')).toHaveCount(0);
  } finally {
    release();
  }
});

test('the first Overview to History click paints a clear page and its records', async ({
  page,
}, testInfo) => {
  type Measurement = { elapsed: number; largestFrameGap: number };
  type ProbeWindow = Window & { historyPaint: Promise<Measurement> };
  await page.goto('/?os=mac');
  // Overview is already reading the same live snapshot that History will display.
  await expect(page.locator('main [data-overview-record]').first()).toBeVisible();
  const openHistory = page.getByRole('button', { name: '全部记录 →', exact: true });
  await openHistory.evaluate((button) => {
    (window as ProbeWindow).historyPaint = new Promise((resolve, reject) => {
      button.addEventListener(
        'click',
        () => {
          const clicked = performance.now();
          let previous = clicked;
          let largestFrameGap = 0;
          const check = () => {
            const now = performance.now();
            largestFrameGap = Math.max(largestFrameGap, now - previous);
            previous = now;
            const surface = document.querySelector<HTMLElement>(
              'main [data-ol-page="history"][data-transition-phase="settled"]',
            );
            const row = surface?.querySelector('[data-history-record]');
            if (surface && row && !surface.inert && row.getBoundingClientRect().height > 0) {
              const pageStyle = getComputedStyle(surface);
              const rowStyle = getComputedStyle(row);
              if (
                pageStyle.opacity === '1' &&
                pageStyle.filter === 'none' &&
                rowStyle.opacity === '1' &&
                // A filled CSS animation can serialize its resting pose as an identity matrix.
                new DOMMatrixReadOnly(rowStyle.transform).isIdentity
              ) {
                // The frame containing these styles can paint before the next callback.
                requestAnimationFrame(() => resolve({ elapsed: now - clicked, largestFrameGap }));
                return;
              }
            }
            if (now - clicked > 5000) reject(new Error('History did not paint a clear record'));
            else requestAnimationFrame(check);
          };
          requestAnimationFrame(check);
        },
        { capture: true, once: true },
      );
    });
  });
  await openHistory.click();
  const paint = await page.evaluate(() => (window as ProbeWindow).historyPaint);
  await testInfo.attach('Overview to History paint timing', {
    body: JSON.stringify(paint),
    contentType: 'application/json',
  });
  // Ignore isolated host stalls; the scrubbed effects below enforce the motion budget.
  if (paint.largestFrameGap <= 34) expect(paint.elapsed).toBeLessThanOrEqual(300);
});

for (const theme of ['light', 'dark'] as const) {
  test(`ready pages keep a painted surface throughout ${theme} navigation`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?os=mac');
    const viewport = page.locator('main .ol-transition-viewport');
    const overview = viewport.locator('[data-ol-page="overview"]');
    const history = viewport.locator('[data-ol-page="history"]');
    await expect(overview).toHaveAttribute('data-transition-phase', 'settled');
    await page.evaluate(() => {
      const animate = Element.prototype.animate;
      (window as NavigationProbe).restoreNavigationClock = () => { Element.prototype.animate = animate; };
      Element.prototype.animate = function (keyframes, options) {
        const animation = animate.call(this, keyframes, options);
        if (this.matches('main [data-transition-phase="enter"]')) {
          animation.pause();
          animation.currentTime = 0;
        }
        return animation;
      };
    });
    try {
      await page.getByRole('button', { name: '历史', exact: true }).click();
      await expect(history).toHaveAttribute('data-transition-phase', 'enter');
      await expect(overview).toHaveCSS('opacity', '1');
      await expect(overview).toHaveAttribute('inert', '');
      const samples = await viewport.evaluate(async (node) => {
        const incoming = node.querySelector<HTMLElement>('[data-transition-phase="enter"]')!;
        const animation = incoming.getAnimations().find((a) => a.id === 'ol-page-enter')!;
        await animation.ready;
        const duration = Number(animation.effect!.getComputedTiming().activeDuration);
        return [0, 0.25, 0.5, 0.75].map((fraction) => {
          animation.currentTime = duration * fraction;
          const surfaces = [...node.children] as HTMLElement[];
          return Math.max(...surfaces.map((surface) => Number(getComputedStyle(surface).opacity)));
        });
      });
      expect(samples).toEqual([1, 1, 1, 1]);
      await history.evaluate((node) => node.getAnimations().forEach((animation) => animation.finish()));
      await expect(history).toHaveAttribute('data-transition-phase', 'settled');
      await expect(overview).toHaveCount(0);
      await expect(history).toHaveCSS('filter', 'none');
    } finally {
      await page.evaluate(() => (window as NavigationProbe).restoreNavigationClock?.());
    }
  });
}

test('fetching and selecting models keeps the form and verify button at the same position', async ({
  page,
}) => {
  const dialog = await settings(page);
  await dialog.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await page.getByRole('button', { name: '添加渠道', exact: true }).click();
  const form = page.getByRole('dialog', { name: '添加渠道', exact: true });
  expect(await form.evaluate((element) => !!element.closest('[inert]'))).toBe(false);
  await form.getByRole('combobox', { name: '供应商', exact: true }).click();
  await page.getByRole('option', { name: 'DeepSeek', exact: true }).click();
  const fetch = form.getByRole('button', { name: '拉取模型', exact: true });
  const verify = form.getByRole('button', { name: '验证', exact: true });
  await verify.scrollIntoViewIfNeeded();
  const initial = (await verify.boundingBox())!;
  const picker = form.getByRole('combobox', { name: '选择一个模型写入上方字段', exact: true });
  await expect(picker).toBeDisabled();
  await fetch.click();
  await expect(picker).toBeEnabled();
  const loaded = (await verify.boundingBox())!;
  expect(Math.abs(initial.y - loaded.y)).toBeLessThan(1);
  const fetchRect = (await fetch.boundingBox())!;
  const pickerRect = (await picker.boundingBox())!;
  // The picker owns the full field column below the fetch row, so its left edge
  // matches endpoint / API key and its width is no longer a fraction of the row.
  expect(Math.abs(pickerRect.x - fetchRect.x)).toBeLessThan(1);
  expect(pickerRect.y).toBeGreaterThan(fetchRect.y + fetchRect.height - 2);
  expect(pickerRect.width).toBeGreaterThan(fetchRect.width * 2);
  await expect(form.locator('.ol-channel-model-fetch [role="status"]')).toContainText('已拉取');
  await picker.click();
  await page.getByRole('option').last().click();
  expect(Math.abs((await verify.boundingBox())!.y - initial.y)).toBeLessThan(1);
  await page.screenshot({ path: 'test-results/models-stable-layout.png' });
});

test('dictation shortcuts hug the key label and coarse wheel scrolling has intermediate positions', async ({
  page,
}) => {
  // Keep the form scrollable even when the page itself fits the default window.
  await page.setViewportSize({ width: 1300, height: 600 });
  const dialog = await settings(page);
  await openMicrophoneSettings(dialog);
  const recorder = dialog.locator('.ol-shortcut-recorder').first();
  const binding = recorder.locator('.ol-shortcut-binding');
  await expect(binding).toContainText('右 Ctrl');
  expect((await binding.boundingBox())!.width).toBeLessThan(140);
  await dialog
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name: '快捷键', exact: true })
    .click();
  await settleNavigation(dialog);
  const scroll = dialog.locator('.ol-settings-scroll[data-transition-phase="settled"]');
  await scroll.evaluate((element) => {
    element.scrollTop = 0;
  });
  const box = (await scroll.boundingBox())!;
  await page.mouse.move(box.x + box.width - 30, box.y + box.height / 2);
  const sampling = scroll.evaluate(async (element) => {
    const values: number[] = [];
    for (let frame = 0; frame < 30; frame++) {
      await new Promise(requestAnimationFrame);
      values.push(element.scrollTop);
    }
    return values;
  });
  await page.mouse.wheel(0, 120);
  const positions = await sampling;
  const end = positions.at(-1)!;
  expect(end).toBeGreaterThan(80);
  expect(positions.some((value) => value > 1 && value < end - 1)).toBe(true);
  expect(positions.every((value, index) => !index || value >= positions[index - 1] - 1)).toBe(true);
});


test('pipeline switches keep the fixed controls and a visible model surface', async ({ page }) => {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  const pipeline = page.locator('.ol-service-pipeline-mode');
  await expect(pipeline).toBeVisible();
  await page.waitForFunction(() => document.querySelector('.ol-settings-scroll')?.getAttribute('data-transition-phase') === 'settled');
  const original = await pipeline.boundingBox();
  await pipeline.evaluate((node) => node.setAttribute('data-stable-pipeline', 'true'));
  const models = page.locator('.ol-services-page > .ol-transition-viewport');
  await models.evaluate((node) => {
    const samples: number[] = [];
    let active = true;
    const sample = () => {
      if (!active) return;
      samples.push(Math.max(0, ...[...node.children].map((surface) => Number(getComputedStyle(surface).opacity))));
      requestAnimationFrame(sample);
    };
    Object.assign(window, { stopPipelineSamples: () => { active = false; return samples; } });
    sample();
  });
  for (const mode of ['多模态模式', 'DeepSeek 大统一', '传统模式', '多模态模式', '传统模式']) {
    await page.getByRole('button', { name: mode, exact: true }).click();
    await expect(page.getByRole('button', { name: mode, exact: true })).toHaveAttribute('aria-pressed', 'true');
  }
  await expect(pipeline).not.toHaveAttribute('aria-busy', 'true');
  await expect(models.locator('[data-transition-phase="settled"]')).toHaveCount(1);
  const samples = await page.evaluate(() => (window as unknown as { stopPipelineSamples(): number[] }).stopPipelineSamples());
  expect(Math.min(...samples)).toBe(1);
  await expect(pipeline).toHaveAttribute('data-stable-pipeline', 'true');
  expect(await pipeline.boundingBox()).toEqual(original);
});
