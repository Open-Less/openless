import { expect, test, type Page } from '@playwright/test';

async function settings(page: Page) {
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(dialog).toHaveCSS('transform', 'none');
  return dialog;
}

test('a pending lazy page keeps the old page visible and exits with blur, scale and overlap', async ({
  page,
}) => {
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
  await page.goto('/?os=mac');
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await request;
  const previous = page.locator('main [data-transition-phase="settled"]');
  await expect(previous.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
  await expect(previous).toHaveCSS('opacity', '1');
  await page.evaluate(() => {
    const animate = Element.prototype.animate;
    (window as any).navigationAnimations = [];
    (window as any).restoreNavigationClock = () => {
      Element.prototype.animate = animate;
    };
    Element.prototype.animate = function (keyframes, options) {
      const animation = animate.call(this, keyframes, options);
      if (
        this.matches('main [data-transition-phase="enter"], main [data-transition-phase="exit"]')
      ) {
        // Sample the native timeline deterministically even when a busy runner skips RAF callbacks.
        animation.pause();
        animation.currentTime = 0;
        (window as any).navigationAnimations.push({ animation, element: this });
      }
      return animation;
    };
  });
  release();
  await page.waitForFunction(() => {
    const active = (window as any).navigationAnimations as { animation: Animation }[];
    return ['ol-page-enter', 'ol-page-exit'].every((id) =>
      active.some(({ animation }) => animation.id === id && animation.playState !== 'idle'),
    );
  });
  const samples = await page.evaluate(async () => {
    const active = (
      (window as any).navigationAnimations as { animation: Animation; element: Element }[]
    ).filter(({ animation }) => animation.playState !== 'idle');
    await Promise.all(active.map(({ animation }) => animation.ready));
    const end = Math.max(
      ...active.map(({ animation }) => Number(animation.effect!.getComputedTiming().endTime)),
    );
    try {
      return Array.from({ length: 20 }, (_, index) => {
        for (const { animation } of active) animation.currentTime = (end * index) / 20;
        return active.map(({ element }) => {
          const style = getComputedStyle(element);
          return {
            phase: element.getAttribute('data-transition-phase'),
            opacity: Number(style.opacity),
            filter: style.filter,
            scale: new DOMMatrixReadOnly(style.transform).m11,
          };
        });
      });
    } finally {
      (window as any).restoreNavigationClock();
      for (const { animation } of active) animation.play();
    }
  });
  await expect(page.locator('[data-ol-page="history"]')).toHaveAttribute(
    'data-transition-phase',
    'settled',
  );
  expect(
    samples.some((frame) =>
      frame.some(
        (node) =>
          node.phase === 'exit' &&
          node.opacity > 0 &&
          node.opacity < 1 &&
          node.scale < 1 &&
          node.filter !== 'none',
      ),
    ),
  ).toBe(true);
  expect(
    samples.some((frame) =>
      frame.some(
        (node) => node.phase === 'enter' && node.opacity > 0 && node.opacity < 1 && node.scale < 1,
      ),
    ),
  ).toBe(true);
  expect(samples.every((frame) => frame.some((node) => node.opacity > 0.01))).toBe(true);
});

test('fetching and selecting models keeps the form and verify button at the same position', async ({
  page,
}) => {
  const dialog = await settings(page);
  await dialog.getByRole('button', { name: 'AI 服务与模型', exact: true }).click();
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
  expect(pickerRect.x).toBeGreaterThan(fetchRect.x + fetchRect.width);
  expect(Math.abs(pickerRect.y - fetchRect.y)).toBeLessThan(4);
  await expect(form.locator('.ol-channel-model-fetch [role="status"]')).toContainText('已拉取');
  await picker.click();
  await page.getByRole('option').last().click();
  expect(Math.abs((await verify.boundingBox())!.y - initial.y)).toBeLessThan(1);
  await page.screenshot({ path: 'test-results/models-stable-layout.png' });
});

test('dictation shortcuts hug the key label and coarse wheel scrolling has intermediate positions', async ({
  page,
}) => {
  const dialog = await settings(page);
  const recorder = dialog.locator('.ol-shortcut-recorder').first();
  const binding = recorder.locator('.ol-shortcut-binding');
  await expect(binding).toContainText('右 Ctrl');
  expect((await binding.boundingBox())!.width).toBeLessThan(140);
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
