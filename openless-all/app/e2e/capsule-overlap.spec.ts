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
    expect(lower.x + lower.width / 2).toBeCloseTo(upper.x + upper.width / 2, 1);
    expect(upper.y + upper.height - lower.y).toBeCloseTo(2, 1);
  };
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
      payload: { text: '新段正在录音，流式原文持续更新。', offset: 0, isFinal: false },
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
  await page.screenshot({ path: info.outputPath('recording-processing-menu.png') });
  await trigger.click();
  await emit('backend:event', {
    sequence: 3,
    sessionId: 'old-a',
    kind: { type: 'dictation_state_changed', payload: { sessionId: 'old-a', phase: 'failed' } },
  });
  await emit('backend:event', {
    sequence: 4,
    sessionId: 'old-a',
    kind: { type: 'transcript_delta', payload: { text: '不得污染新段', offset: 0, isFinal: true } },
  });
  await expect(page.locator('.ol-capsule-transcript-rail')).not.toContainText('不得污染');
  await emit('dictation:jobs', [{ sessionId: 'old-b' }]);
  await assertAnchors();
  await lobe.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActions.at(-1)))
    .toBe('cancel_dictation');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.dictationActionArgs.at(-1)))
    .toEqual({ sessionId: 'old-b' });
  await expect(body).toHaveAttribute('data-recording', 'true');
});
