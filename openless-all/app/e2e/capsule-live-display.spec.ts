import { expect, test } from '@playwright/test';

const fixture = 'http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html';

for (const rail of [false, true]) {
  test(`hidden mirror cleanup cannot restart the live ${rail ? 'transcript rail' : 'capsule'}`, async ({
    page,
  }) => {
    const label = rail ? 'capsule-rail' : 'capsule';
    const mirror = rail ? 'capsule-rail-mirror-0' : 'capsule-mirror-0';
    await page.goto(`${fixture}?os=win&transcript&label=${label}&${rail ? 'rail' : 'full'}`);
    await page.waitForFunction(() => (window as any).fixture.hasListener('backend:event'));
    await page.evaluate((label) => {
      (window as any).fixture.emitTo(label, 'capsule:state', {
        state: 'recording',
        sessionId: 'recording-a',
        level: 0.2,
      });
    }, label);
    const view = page.locator(rail ? '.ol-capsule-transcript-overlay' : '.ol-capsule-pill');
    if (!rail) {
      await expect(view).toHaveAttribute('data-recording', 'true');
      await view.evaluate((node) => node.setAttribute('data-test-retained', 'true'));
    }
    for (let i = 1; i <= 12; i += 1) {
      await page.evaluate(
        ({ label, mirror, i }) => {
          const bridge = (window as any).fixture;
          bridge.emitTo(label, 'capsule:state', {
            state: 'recording',
            sessionId: 'recording-a',
            level: 0.2,
          });
          bridge.emit('backend:event', {
            sequence: i,
            sessionId: 'recording-a',
            kind: {
              type: 'transcript_delta',
              payload: { text: 'x'.repeat(i), offset: 0, isFinal: true },
            },
          });
          bridge.emitTo(mirror, 'capsule:state', { state: 'idle', sessionId: 'recording-a' });
        },
        { label, mirror, i },
      );
      if (rail) {
        await expect(view).toHaveAttribute('data-transcript-visible', 'true');
        await expect(view.getByRole('status')).toHaveText('x'.repeat(i));
        const stage = view.locator('.ol-live-transcript-stage');
        if (i === 1)
          await stage.evaluate((node) => node.setAttribute('data-test-retained', 'true'));
        await expect(stage).toHaveAttribute('data-test-retained', 'true');
      } else {
        await expect(view).toHaveAttribute('data-recording', 'true');
        await expect(view).toHaveAttribute('data-leaving', 'false');
        await expect(view).toHaveAttribute('data-test-retained', 'true');
      }
    }
    if (!rail) {
      await expect
        .poll(() => page.evaluate(() => (window as any).fixture.transcriptVisibility))
        .toEqual([false, true]);
    }
  });
}

test('a retained mirror never drives the primary native transcript rail', async ({ page }) => {
  await page.goto(`${fixture}?full&os=win&transcript&label=capsule-mirror-0`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('backend:event'));
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emitTo('capsule-mirror-0', 'capsule:state', {
      state: 'recording',
      sessionId: 'recording-a',
    });
    bridge.emit('backend:event', {
      sequence: 1,
      sessionId: 'recording-a',
      kind: { type: 'transcript_delta', payload: { text: 'example', offset: 0, isFinal: true } },
    });
  });
  await expect(page.locator('.ol-capsule-pill')).toHaveAttribute('data-recording', 'true');
  await page.evaluate(() =>
    (window as any).fixture.emitTo('capsule-mirror-0', 'capsule:state', {
      state: 'idle',
      sessionId: 'recording-a',
    }),
  );
  await expect(page.locator('.ol-capsule-pill')).toHaveAttribute('data-leaving', 'true');
  expect(await page.evaluate(() => (window as any).fixture.transcriptVisibility)).toEqual([]);
});
