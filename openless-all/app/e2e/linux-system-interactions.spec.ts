import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('Linux history/vocabulary copy uses the current native window, not fcitx5', async ({
  page,
}) => {
  await page.goto(`${fixtureOrigin}/?bridge=http&window=capsule`);
  const result = await page.evaluate(async () => {
    const copied: string[] = [];
    window.__openlessCopyText = async (text) => {
      copied.push(text);
    };
    const path = '/src/api/files.ts';
    const files = await import(/* @vite-ignore */ path);
    await files.copyTextToClipboard('历史记录 中文\nsecond line');
    window.__openlessCopyText = async () => {
      throw new Error('native clipboard denied');
    };
    let error = '';
    try {
      await files.copyTextToClipboard('must not report success');
    } catch (reason) {
      error = String(reason);
    }
    return { copied, error };
  });
  expect(result.copied).toEqual(['历史记录 中文\nsecond line']);
  expect(result.error).toContain('native clipboard denied');
});

test('Linux uses a view-local input-region bridge, including empty cleanup', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/?bridge=http&window=capsule`);
  const writes = await page.evaluate(async () => {
    const writes: unknown[] = [];
    window.__openlessSetInputRegions = (rects) => {
      writes.push(rects);
    };
    const path = '/src/runtime/windows/capsuleInput.ts';
    const module = await import(/* @vite-ignore */ path);
    await module.setCapsuleInteractiveRegions([{ left: 20, top: 10, width: 40, height: 30 }]);
    await module.setCapsuleInteractiveRegions([]);
    return writes;
  });
  expect(writes).toEqual([[{ left: 20, top: 10, width: 40, height: 30 }], []]);
});

test('native Linux main frame is square without changing card corners', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/?bridge=http`);
  // Use the production WindowChrome with its real inline CSS shape. The global
  // continuous-corners selector used to override this zero radius with !important.
  await page.evaluate(() => {
    const chrome = document.createElement('div');
    chrome.className = 'ol-winchrome';
    chrome.style.borderRadius = 'var(--ol-window-shell-radius)';
    chrome.style.setProperty('--ol-window-shell-radius', '0px');
    const card = document.createElement('div');
    card.className = 'ol-card';
    chrome.append(card);
    document.body.append(chrome);
  });
  await expect(page.locator('.ol-winchrome').last()).toHaveCSS('border-top-left-radius', '0px');
  await expect(page.locator('.ol-winchrome').last()).toHaveCSS('border-top-right-radius', '0px');
  await expect(page.locator('.ol-winchrome .ol-card').last()).not.toHaveCSS(
    'border-top-left-radius',
    '0px',
  );
});

test('Linux capsule publishes only visible controls, not the overlay canvas', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=linux`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() => {
    (window as any).fixture.emit('capsule:state', {
      state: 'recording',
      sessionId: 'linux-test',
      level: 0.3,
    });
  });
  await expect(page.locator('.ol-default-action').first()).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() => {
        const regions = (window as any).fixture.inputRegions;
        const button = document.querySelector('.ol-default-action')!.getBoundingClientRect();
        const x = button.left + button.width / 2,
          y = button.top + button.height / 2;
        return (
          regions.length > 0 &&
          regions.some(
            (r: any) => x >= r.left && x <= r.left + r.width && y >= r.top && y <= r.top + r.height,
          ) &&
          !regions.some(
            (r: any) => 1 >= r.left && 1 <= r.left + r.width && 1 >= r.top && 1 <= r.top + r.height,
          )
        );
      }),
    )
    .toBe(true);
  await page.evaluate(() => (window as any).fixture.emit('capsule:state', { state: 'idle' }));
  await expect.poll(() => page.evaluate(() => (window as any).fixture.inputRegions)).toEqual([]);
});
