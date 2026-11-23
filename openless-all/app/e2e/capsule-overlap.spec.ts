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
    expect(upper.height).toBeCloseTo(22, 1);
    expect(lower.y).toBeCloseTo(anchor!.y, 1);
    expect(upper.x + upper.width / 2 - (lower.x + lower.width / 2)).toBeCloseTo(0, 1);
    expect(upper.y + upper.height - lower.y).toBeCloseTo(4, 1);
  };
  await expect(lobe).toContainText('处理中');
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
  await expect.poll(() => page.evaluate(() => {
    const rear = document.querySelector('.ol-processing-satellite')!.getBoundingClientRect();
    const body = document.querySelector('.ol-default-capsule')!.getBoundingClientRect();
    const x = rear.left + rear.width / 2, y = (rear.top + body.top) / 2;
    return (window as any).fixture.inputRegions.some((r: any) =>
      x >= r.left && x <= r.left + r.width && y >= r.top && y <= r.top + r.height);
  })).toBe(true);
  await lobe.click({ position: { x: 52, y: 10 } });
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActions.at(-1)))
    .toBe('cancel_dictation');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'old-b' });
  await expect(body).toHaveAttribute('data-recording', 'true');
  await body.locator('.ol-default-action').nth(1).click();
  await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'recording-a' });
});

for (const reducedMotion of ['no-preference', 'reduce'] as const) {
  test(`processing wave settles into slow connection and cancels its owner: ${reducedMotion}`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion });
    await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
    await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
    await page.evaluate(() => (window as any).fixture.emit('capsule:state', {
      state: 'polishing', sessionId: 'processing-a', translation: false,
    }));
    const pill = page.locator('.ol-default-capsule');
    await expect(pill).toHaveCSS('width', '116px');
    await expect(pill.locator('.ol-capsule-wave-progress')).toHaveCount(1);
    await expect(pill).toHaveAttribute('data-slow-connection', 'false');
    if (reducedMotion === 'reduce') {
      await expect(pill.locator('.ol-capsule-wave-progress')).toHaveCSS('animation-name', 'none');
    }
    await expect(pill).toHaveAttribute('data-slow-connection', 'true', { timeout: 6000 });
    await expect(pill.locator(reducedMotion === 'reduce'
      ? '.ol-capsule-slow-static' : '.ol-capsule-marquee')).toBeVisible();
    await pill.locator('.ol-default-processing-cancel').click();
    await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
      .toEqual({ sessionId: 'processing-a' });
    await page.evaluate(() => (window as any).fixture.emit('capsule:state', {
      state: 'recording', sessionId: 'recording-b', translation: false,
    }));
    await expect(pill).toHaveAttribute('data-slow-connection', 'false');
    await expect(pill.locator('.ol-capsule-marquee')).toHaveCount(0);
  });
}

test('queued foreground processing starts a fresh phase without remounting the capsule', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() => {
    const f = (window as any).fixture;
    f.emit('dictation:jobs', [{ sessionId: 'queued-a' }, { sessionId: 'queued-b' }]);
    f.emit('capsule:state', { state: 'polishing', sessionId: 'queued-a', translation: false });
  });
  const pill = page.locator('.ol-default-capsule');
  await expect(pill).toHaveAttribute('data-slow-connection', 'true', { timeout: 6000 });
  await page.evaluate(() => {
    const pill = document.querySelector('.ol-default-capsule')!;
    (pill as any).previousWave = pill.querySelector('.ol-capsule-wave-layer');
    (pill as any).retainedOwner = true;
    const f = (window as any).fixture;
    f.emit('dictation:jobs', [{ sessionId: 'queued-b' }]);
    f.emit('capsule:state', { state: 'polishing', sessionId: 'queued-b', translation: false });
  });
  await expect(pill).toHaveAttribute('data-slow-connection', 'false');
  expect(await pill.evaluate((node) => (node as any).retainedOwner &&
    (node as any).previousWave !== node.querySelector('.ol-capsule-wave-layer'))).toBe(true);
  await expect(pill.locator('.ol-capsule-marquee')).toHaveCount(0);
  await expect(pill).toHaveAttribute('data-slow-connection', 'true', { timeout: 6000 });
  await pill.locator('.ol-default-processing-cancel').click();
  await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'queued-b' });
});

for (const locale of ['en', 'de', 'fr']) {
  test(`reduced motion keeps slow processing visually readable at 125%: ${locale}`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
    await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
    // The fixture uses the same i18next instance as the real capsule.
    await page.evaluate(async (locale) => {
      const { setLocalePreference } = await import('/src/i18n/index.ts');
      await setLocalePreference(locale as 'en' | 'de' | 'fr');
      (window as any).fixture.emit('capsule:state', { state: 'polishing', sessionId: 'slow-locale' });
    }, locale);
    await page.addStyleTag({ content: '.ol-default-status { font-size: 15px; }' });
    const text = page.locator('.ol-capsule-slow-static');
    await expect(text).toHaveText({ en: 'thinking', de: 'denkt nach', fr: 'réflexion' }[locale]!, { timeout: 6000 });
    expect(await text.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await expect(page.locator('.ol-capsule-marquee')).toHaveCount(0);
    await expect(page.locator('.ol-translation-language-collapse')).toHaveCSS('transition-duration', '0s');
    await expect(page.locator('.ol-default-capsule')).toHaveCSS('width', '116px');
  });
}

test('processing foreground publishes the settled rear hit region and cancels only its job', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() => {
    const f = (window as any).fixture;
    f.emit('capsule:state', { state: 'polishing', sessionId: 'front-processing', translation: false });
    f.emit('dictation:jobs', [{ sessionId: 'background-processing', translationActive: true }]);
  });
  const rear = page.locator('.ol-processing-satellite');
  await expect(rear).toBeVisible();
  await rear.evaluate((node) => Promise.all(node.getAnimations().map((animation) => animation.finished)));
  await expect.poll(() => page.evaluate(() => {
    const rear = document.querySelector('.ol-processing-satellite')!.getBoundingClientRect();
    const front = document.querySelector('.ol-default-capsule')!.getBoundingClientRect();
    return (window as any).fixture.inputRegions.some((r: any) =>
      r.top <= rear.top + 1 && r.top + r.height >= front.top - 1);
  })).toBe(true);
  const foreground = page.locator('.ol-default-capsule');
  const r = (await rear.boundingBox())!, f = (await foreground.boundingBox())!;
  expect(r.height).toBeCloseTo(22, 1);
  expect(r.x + r.width / 2).toBeCloseTo(f.x + f.width / 2, 1);
  expect(r.y + r.height - f.y).toBeCloseTo(4, 1);
  await rear.click({ position: { x: r.width / 2, y: 8 } });
  await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'background-processing' });
  await foreground.locator('.ol-default-processing-cancel').click();
  await expect.poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'front-processing' });
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
    await expect(badge).toHaveAttribute('data-visible', 'false');
    await expect(page.locator('.ol-processing-satellite')).toContainText('正在翻译');
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

test('empty recognition shows its concise status', async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/capsule-motion.html?full&os=mac`);
  await page.waitForFunction(() => (window as any).fixture.hasListener('dictation:jobs'));
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'error',
      sessionId: 'empty-a',
      message: 'empty_transcript',
    }),
  );
  const label = page.locator('.ol-default-status');
  await expect(label).toHaveText('空稿 · 未识别到语音');
  await expect
    .poll(() => label.evaluate((element) => element.scrollWidth <= element.clientWidth))
    .toBe(true);
});
