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

for (const theme of ['light', 'dark'] as const) {
  test(`a pending page leaves its layout in place, then fades through the ${theme} surface`, async ({
    page,
  }, testInfo) => {
    let release!: () => void;
    let requested!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const request = new Promise<void>((resolve) => {
      requested = resolve;
    });
    await page.route(/\/assets\/History-[^/]+\.js/, async (route) => {
      requested();
      await gate;
      await route.continue();
    });
    await page.emulateMedia({ colorScheme: theme });
    await page.goto('/?os=mac');
    await expect(page.locator('html')).toHaveCSS('color-scheme', theme);
    const viewport = page.locator('main .ol-transition-viewport');
    const overview = viewport.locator('[data-ol-page="overview"]');
    const history = viewport.locator('[data-ol-page="history"]');
    try {
      await page.getByRole('button', { name: '历史', exact: true }).click();
      await request;
      await expect(overview).toHaveAttribute('data-transition-phase', 'retained');
      await expect(overview.getByText('今日概览', { exact: true })).toBeVisible();
      await expect(overview).toHaveCSS('opacity', '1');
      await expect(overview).toHaveCSS('overflow', 'hidden');
      await expect(overview).toHaveClass(/\bol-page-fixed\b/);
      await expect(history).toHaveAttribute('data-transition-phase', 'waiting');
      await expect(history).toHaveCSS('opacity', '0');
      await page.evaluate(() => {
        const animate = Element.prototype.animate;
        (window as NavigationProbe).restoreNavigationClock = () => {
          Element.prototype.animate = animate;
        };
        Element.prototype.animate = function (keyframes, options) {
          const animation = animate.call(this, keyframes, options);
          if (
            this.matches(
              'main [data-transition-phase="enter"], main [data-transition-phase="exit"]',
            )
          ) {
            // Sample the real effect without depending on a busy runner painting every frame.
            animation.pause();
            animation.currentTime = 0;
          }
          return animation;
        };
      });
      release();

      const sample = async (surface: typeof overview, phase: 'exit' | 'enter') => {
        await expect(surface).toHaveAttribute('data-transition-phase', phase);
        return surface.evaluate(async (element, id) => {
          const animation = element.getAnimations().find((candidate) => candidate.id === id)!;
          await animation.ready;
          const duration = Number(animation.effect!.getComputedTiming().activeDuration);
          return [0, 0.35, 0.7].map((fraction) => {
            animation.currentTime = duration * fraction;
            const style = getComputedStyle(element);
            return {
              opacity: Number(style.opacity),
              blur: parseFloat(style.filter.match(/blur\(([\d.]+)px\)/)?.[1] ?? '0'),
              scale: new DOMMatrixReadOnly(style.transform).m11,
            };
          });
        }, `ol-page-${phase}`);
      };
      const exit = await sample(overview, 'exit');
      await page.screenshot({
        path: testInfo.outputPath(`acceptance-transition-${theme}-exit.png`),
      });
      await expect(history).toHaveAttribute('data-transition-phase', 'waiting');
      await expect(history).toHaveCSS('opacity', '0');
      expect(exit[0].opacity).toBe(1);
      expect(exit[1].opacity).toBeGreaterThan(exit[2].opacity);
      expect(exit[0].blur).toBeLessThan(exit[1].blur);
      expect(exit[1].blur).toBeLessThan(exit[2].blur);
      await overview.evaluate((el) =>
        el
          .getAnimations()
          .find((a) => a.id === 'ol-page-exit')!
          .finish(),
      );

      await expect(history).toHaveAttribute('data-transition-phase', 'enter');
      await expect(overview).toHaveCSS('opacity', '0');
      await expect(history).toHaveCSS('opacity', '0');
      await expect(viewport).toHaveCSS(
        'background-color',
        theme === 'light' ? 'rgb(255, 255, 255)' : 'rgb(28, 28, 31)',
      );
      await page.screenshot({
        path: testInfo.outputPath(`acceptance-transition-${theme}-surface.png`),
      });
      const enter = await sample(history, 'enter');
      await page.screenshot({
        path: testInfo.outputPath(`acceptance-transition-${theme}-enter.png`),
      });
      expect(enter[0].opacity).toBe(0);
      expect(enter[1].opacity).toBeLessThan(enter[2].opacity);
      expect(enter[0].blur).toBeGreaterThan(enter[1].blur);
      expect(enter[1].blur).toBeGreaterThan(enter[2].blur);
      expect([...exit, ...enter].map(({ scale }) => scale)).toEqual(Array(6).fill(1));
      await history.evaluate((el) =>
        el
          .getAnimations()
          .find((a) => a.id === 'ol-page-enter')!
          .play(),
      );
      await expect(history).toHaveAttribute('data-transition-phase', 'settled');
      await expect(history).toHaveCSS('opacity', '1');
      await expect(history).toHaveCSS('filter', 'none');
      await expect(overview).toHaveCount(0);
    } finally {
      release();
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
