import { expect, test } from '@playwright/test';

test('the real Mac capsule keeps controls interactive during continuous animation', async ({
  page,
}) => {
  await page.goto('http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html?full&os=mac');
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() => {
    const fixture = (window as any).fixture;
    fixture.stopTicks = false;
    const tick = () => {
      document.body.style.setProperty('--fixture-frame', String(performance.now()));
      if (!fixture.stopTicks) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    fixture.emit('capsule:state', {
      state: 'recording',
      level: 0.25,
      translation: true,
      sessionId: 'recording-a',
    });
  });
  const cancel = page.locator('.ol-default-action').first();
  const confirm = page.locator('.ol-default-action').nth(1);
  await expect(cancel).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() => {
        const regions = (window as any).fixture.inputRegions;
        const nodes = [
          ...document.querySelectorAll(
            '.ol-default-action:not([aria-hidden="true"]), .ol-classic-translation button',
          ),
        ];
        return (
          nodes.length === 3 &&
          nodes.every((node) => {
            const r = node.getBoundingClientRect(),
              x = r.left + r.width / 2,
              y = r.top + r.height / 2;
            return regions.some(
              (region: any) =>
                x >= region.left &&
                x <= region.left + region.width &&
                y >= region.top &&
                y <= region.top + region.height,
            );
          })
        );
      }),
    )
    .toBe(true);
  await confirm.click();
  await cancel.click();
  await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActions)).toEqual([
    'stop_dictation',
    'cancel_dictation',
  ]);
  await page.locator('.ol-classic-translation [role="combobox"]').click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page
    .getByRole('option')
    .filter({ hasText: /日语|日本語/ })
    .first()
    .click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.targetChanges.length))
    .toBe(1);
  await expect(page.getByRole('listbox')).toHaveCount(0);
  await page.evaluate(() => {
    (window as any).fixture.stopTicks = true;
  });
});
