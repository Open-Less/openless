import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test('processing lobe stays joined to recording through transcript, menu and job changes', async ({
  page,
}, info) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac&transcript`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('dictation:jobs'));
  const emit = async (event: string, payload: unknown) =>
    page.evaluate(({ event, payload }) => (window as any).fixture.emit(event, payload), {
      event,
      payload,
    });
  await emit('capsule:state', {
    state: 'recording',
    sessionId: 'recording-a',
    level: 0.3,
    translation: true,
  });
  await emit('dictation:jobs', [{ sessionId: 'old-a' }]);
  const body = page.locator('.ol-default-capsule');
  const lobe = page.locator('.ol-processing-satellite');
  await expect(body).toHaveCSS('width', '116px');
  await expect(lobe).toBeVisible();
  const anchor = await body.boundingBox();
  const assertAnchors = async () => {
    const lower = (await body.boundingBox())!;
    const upper = (await lobe.boundingBox())!;
    expect(lower.y).toBeCloseTo(anchor!.y, 1);
    expect(upper.x + upper.width / 2 - (lower.x + lower.width / 2)).toBeCloseTo(10, 1);
    expect(upper.y + upper.height - lower.y).toBeCloseTo(20, 1);
  };
  await expect(lobe).toContainText('正在转录');
  await page.waitForTimeout(250);
  await page.screenshot({ path: info.outputPath('recording-processing.png') });
  await assertAnchors();
  await emit('backend:event', {
    sequence: 1,
    sessionId: 'recording-a',
    kind: {
      type: 'dictation_state_changed',
      payload: { sessionId: 'recording-a', phase: 'recording' },
    },
  });
  await emit('backend:event', {
    sequence: 2,
    sessionId: 'recording-a',
    kind: {
      type: 'transcript_delta',
      payload: {
        text: '新段正在录音，流式原文持续更新。',
        offset: 0,
        isFinal: false,
      },
    },
  });
  await expect(page.locator('.ol-capsule-transcript-rail')).toBeVisible();
  await assertAnchors();
  const trigger = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(trigger).toBeEnabled();
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await assertAnchors();
  await emit('dictation:jobs', [
    { sessionId: 'old-a' },
    { sessionId: 'old-b' },
    { sessionId: 'old-c' },
  ]);
  await expect(lobe).toContainText('3');
  await expect(page.getByRole('listbox')).toBeVisible();
  await assertAnchors();
  await page.screenshot({
    path: info.outputPath('recording-processing-menu.png'),
  });
  await trigger.click();
  await emit('backend:event', {
    sequence: 3,
    sessionId: 'old-a',
    kind: {
      type: 'dictation_state_changed',
      payload: { sessionId: 'old-a', phase: 'failed' },
    },
  });
  await emit('backend:event', {
    sequence: 4,
    sessionId: 'old-a',
    kind: {
      type: 'transcript_delta',
      payload: { text: '不得污染新段', offset: 0, isFinal: true },
    },
  });
  await expect(page.locator('.ol-capsule-transcript-rail')).not.toContainText('不得污染');
  await emit('dictation:jobs', [{ sessionId: 'old-b' }]);
  await assertAnchors();
  await lobe.click({ position: { x: 52, y: 10 } });
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActions.at(-1)))
    .toBe('cancel_dictation');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'old-b' });
  await expect(body).toHaveAttribute('data-recording', 'true');
});

test('translation keeps its element and moves continuously when streaming starts', async ({
  page,
}, info) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac&transcript`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('dictation:jobs'));
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'recording',
      sessionId: 'recording-a',
      translation: true,
      level: 0.3,
    }),
  );
  const badge = page.locator('.ol-classic-translation');
  await expect(badge).toHaveText(/正在翻译/);
  await page.waitForTimeout(300);
  const trace = await page.evaluate(async () => {
    const fixture = (window as any).fixture;
    const element = document.querySelector('.ol-classic-translation')!;
    const samples: { time: number; y: number; same: boolean }[] = [];
    const start = performance.now();
    const sample = () =>
      samples.push({
        time: performance.now() - start,
        y: element.getBoundingClientRect().y,
        same: element === document.querySelector('.ol-classic-translation'),
      });
    sample();
    fixture.emit('backend:event', {
      sequence: 1,
      sessionId: 'recording-a',
      kind: {
        type: 'dictation_state_changed',
        payload: { sessionId: 'recording-a', phase: 'recording' },
      },
    });
    fixture.emit('backend:event', {
      sequence: 2,
      sessionId: 'recording-a',
      kind: {
        type: 'transcript_delta',
        payload: { text: '流式原文出现时角标平滑移动', offset: 0, isFinal: false },
      },
    });
    await new Promise<void>((resolve) => {
      const tick = () => {
        sample();
        if (performance.now() - start < 350) requestAnimationFrame(tick);
        else resolve();
      };
      requestAnimationFrame(tick);
    });
    return samples;
  });
  expect(trace.every((sample) => sample.same)).toBe(true);
  const first = trace[0].y,
    last = trace.at(-1)!.y;
  expect(first - last).toBeCloseTo(60, 0);
  expect(trace.some((sample) => sample.y < first - 2 && sample.y > last + 2)).toBe(true);
  expect(
    Math.max(...trace.slice(1).map((sample, index) => Math.abs(sample.y - trace[index].y))),
  ).toBeLessThan(40);
  await info.attach('translation-motion', {
    body: JSON.stringify(trace, null, 2),
    contentType: 'application/json',
  });
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'recording',
      sessionId: 'recording-a',
      translation: false,
      level: 0.3,
    }),
  );
  for (const phase of ['completed', 'failed', 'cancelled']) {
    await page.evaluate((phase) => {
      const f = (window as any).fixture;
      f.emit('dictation:jobs', [{ sessionId: 'old-translation', translationActive: true }]);
      f.emit('backend:event', {
        sequence: 3,
        sessionId: 'old-translation',
        kind: { type: 'dictation_state_changed', payload: { sessionId: 'old-translation', phase } },
      });
    }, phase);
    await expect(page.locator('.ol-default-capsule')).toHaveAttribute('data-recording', 'true');
    await expect(badge).toHaveAttribute('data-visible', 'true');
  }
  await page.evaluate(() => (window as any).fixture.emit('dictation:jobs', []));
  await expect(badge).toHaveAttribute('data-visible', 'false');
  await expect(page.locator('.ol-default-capsule')).toHaveAttribute('data-recording', 'true');
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'transcribing',
      sessionId: 'recording-a',
      translation: true,
    }),
  );
  await expect(badge).toHaveAttribute('data-visible', 'true');
  await expect(badge).toHaveText('正在翻译');
  await expect(page.getByRole('combobox')).toHaveCount(0);
  await page.screenshot({ path: info.outputPath('translation-after-stop.png') });
});

test('empty recognition preserves its recovery message', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('dictation:jobs'));
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'error',
      sessionId: 'empty-a',
      message: '未识别到语音，录音已保留，可在历史中重新转录',
    }),
  );
  const label = page.locator('.ol-default-status');
  await expect(label).toContainText('可在历史中重新转录');
  await expect
    .poll(() => label.evaluate((element) => element.scrollWidth <= element.clientWidth))
    .toBe(true);
});
