import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';

const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;

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

test('tabular transcript digits use shaped advances and never overlap', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(`${fixture}?os=win&transcript&label=capsule-rail&rail`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('backend:event'));
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emitTo('capsule-rail', 'capsule:state', {
      state: 'recording',
      sessionId: 'recording-a',
    });
    bridge.emit('backend:event', {
      sequence: 1,
      sessionId: 'recording-a',
      kind: {
        type: 'transcript_delta',
        payload: { text: '11110000', offset: 0, isFinal: true },
      },
    });
  });
  const glyphs = page.locator('.ol-live-transcript-char');
  await expect(glyphs).toHaveCount(8);
  await expect(glyphs.first()).toHaveCSS('filter', 'none');
  await expect(glyphs.first()).toHaveCSS('will-change', 'auto');
  const bounds = await glyphs.evaluateAll((nodes) =>
    nodes.map((node) => {
      const r = node.getBoundingClientRect();
      return { left: r.left, right: r.right, width: r.width };
    }),
  );
  expect(Math.abs(bounds[0].width - bounds[4].width)).toBeLessThan(0.1);
  for (let i = 0; i < bounds.length - 1; i++)
    expect(bounds[i + 1].left - bounds[i].right).toBeGreaterThan(0.5);
});

test('Beta WebView traces sampled caption acceptance and callbacks across a long stream and successor', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(`${fixture}?os=mac&full&transcript&label=capsule&captionTrace`);
  await page.waitForFunction(() => {
    const bridge = (window as any).fixture;
    return bridge.hasListener('backend:event') && bridge.captionTraceReads === 1;
  });
  const receipts = () => page.evaluate(() => (window as any).fixture.captionTraceReports);
  const caption = page.locator('.ol-live-transcript-sr');
  await page.clock.install();
  await page.evaluate(() => {
    (window as any).fixture.emitTo('capsule', 'capsule:state', {
      state: 'recording',
      sessionId: 'recording-a',
    });
  });
  const expected = [];
  for (const [first, last] of [[1, 215], [216, 430], [431, 650]]) {
    if (first !== 1) await page.clock.fastForward(43_000);
    await page.evaluate(({ first, last }) => {
      const bridge = (window as any).fixture;
      for (let sequence = first; sequence <= last; sequence++) {
        bridge.emit('backend:event', {
          sequence,
          sessionId: 'recording-a',
          kind: {
            type: 'transcript_delta',
            payload: { text: `${sequence * 200}ms ${'x'.repeat(sequence)}`, offset: 0, isFinal: false },
          },
        });
      }
    }, { first, last });
    await expect(caption).toHaveText(`${last * 200}ms ${'x'.repeat(last)}`);
    expected.push(
      { sessionId: 'recording-a', sequence: first, stage: 'accepted' },
      { sessionId: 'recording-a', sequence: last, stage: 'render_callback' },
    );
    await expect.poll(receipts).toEqual(expected);
  }

  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emitTo('capsule', 'capsule:state', { state: 'transcribing', sessionId: 'recording-a' });
    bridge.emitTo('capsule', 'capsule:state', { state: 'error', sessionId: 'recording-a' });
  });
  await expect(caption).toHaveCount(0);
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emitTo('capsule', 'capsule:state', { state: 'recording', sessionId: 'recording-b' });
    for (const [sessionId, sequence, text] of [
      ['recording-b', 651, 'new session'],
      ['recording-a', 652, 'late old delta'],
    ]) {
      bridge.emit('backend:event', { sessionId, sequence, kind: {
        type: 'transcript_delta', payload: { text, offset: 0, isFinal: false },
      } });
    }
  });
  await expect(caption).toHaveText('new session');
  await expect.poll(receipts).toEqual([
    ...expected,
    { sessionId: 'recording-b', sequence: 651, stage: 'accepted' },
    { sessionId: 'recording-a', sequence: 652, stage: 'rejected' },
    { sessionId: 'recording-b', sequence: 651, stage: 'render_callback' },
  ]);
});

test('Beta WebView renders captions with tracing disabled by default', async ({ page }) => {
  await page.goto(`${fixture}?os=mac&full&transcript&label=capsule`);
  await page.waitForFunction(() => {
    const bridge = (window as any).fixture;
    return bridge.hasListener('backend:event') && bridge.captionTraceReads === 1;
  });
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emitTo('capsule', 'capsule:state', { state: 'recording', sessionId: 'recording-a' });
    bridge.emit('backend:event', { sessionId: 'recording-a', sequence: 1, kind: {
      type: 'transcript_delta', payload: { text: 'fixture caption', offset: 0, isFinal: false },
    } });
  });
  await expect(page.locator('.ol-live-transcript-sr')).toHaveText('fixture caption');
  expect(await page.evaluate(() => (window as any).fixture.captionTraceReports)).toEqual([]);
});
