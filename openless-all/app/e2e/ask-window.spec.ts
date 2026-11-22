import { expect, test } from '@playwright/test';
import type {} from './fixtures/ask-window';
import { fixtureOrigin } from './support/server';

test.use({ viewport: { width: 800, height: 920 }, locale: 'en-US' });

test.beforeEach(async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/ask-window.html`);
  await expect(page.getByRole('textbox', { name: 'Ask anything' })).toBeFocused();
});

const promptInput = '[data-slot="prompt-box-input"]';

test('a recovered question route expands one frame without replacing the editor', async ({
  page,
}) => {
  await expect(page.locator('#ask-root')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const prompt = page.locator(promptInput);
  const box = page.locator('[data-slot="prompt-box"]');
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Close Ask Conversation' })).toHaveCount(0);
  const originalEditor = await prompt.elementHandle();
  const collapsed = (await box.boundingBox())!;
  await page.evaluate(() => {
    window.askFixture.route = null;
  });
  await prompt.fill('hi');
  await prompt.press('Enter');
  await expect(page.getByRole('region', { name: 'Ask result' })).toHaveCount(0);
  await expect(page.getByText('Working', { exact: true })).toHaveCount(0);
  await expect(page.locator('.user-bubble')).toHaveCount(0);
  await page.evaluate(() => window.askFixture.resolve('answer', false));
  const backdrop = page.locator('.ask-expansion-backdrop');
  const frame = page.locator('.ask-presentation');
  await expect(frame).toHaveAttribute('data-expanded', 'true');
  await expect(backdrop).toHaveCSS('border-radius', '24px');
  expect(
    await frame.evaluate((e) => parseFloat(e.style.getPropertyValue('--ask-start-width'))),
  ).toBeCloseTo(collapsed.width);
  await expect(page.locator('.user-bubble')).toHaveText('hi');
  await expect(page.locator('.effort-label')).toHaveCount(0);
  await expect(prompt).toBeFocused();
  expect(await prompt.evaluate((node, original) => node === original, originalEditor)).toBe(true);
  await expect.poll(async () => (await backdrop.boundingBox())!.height).toBeGreaterThan(900);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({ type: 'answerDelta', turnId, messageId: 'answer', text: 'Hello' }, false);
    fixture.push({ type: 'answerDelta', turnId, messageId: 'answer', text: ' there.' });
  });
  await expect(page.locator('.assistant-text')).toHaveText('Hello there.');
  await prompt.fill('keep this draft');
  await prompt.dispatchEvent('keydown', { key: 'Escape', isComposing: true });
  expect(await page.evaluate(() => window.askFixture.hideCount)).toBe(0);
  await prompt.press('Escape');
  await expect.poll(() => page.evaluate(() => window.askFixture.liveListeners())).toBe(0);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'turnFinished', turnId: 'turn-1', status: 'completed' });
    fixture.activate();
  });
  await expect(backdrop).toHaveCount(0);
  await expect(prompt).toHaveValue('keep this draft');
  expect(
    await page.evaluate(() => [window.askFixture.hideCount, window.askFixture.expandCount]),
  ).toEqual([1, 1]);
});

test('selection does not expand Ask until the user submits a selected-text question', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.evaluate(() => {
    window.askFixture.route = 'selectionAnswer';
    window.askFixture.activate();
  });
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  await prompt.fill('What does the selected sentence mean?');
  await prompt.press('Enter');
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(1);
  await expect(page.locator('.user-bubble')).toHaveText('What does the selected sentence mean?');
  await expect(prompt).toBeFocused();
  await page.getByRole('button', { name: 'Close Ask Conversation' }).click();
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.cancelled)).toHaveLength(0);
});

test('same-frame attachment references, full selection, IME, caret, and attachment-only retry', async ({
  page,
}) => {
  const box = page.locator('[data-slot="prompt-box"]');
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.evaluate(() => window.askFixture.addInitialCards());
  await expect(box.getByRole('img', { name: 'image.png' })).toBeVisible();
  await expect(box.getByText('Image #1', { exact: true })).toHaveCount(2);
  await expect(box.getByText('PDF · 3 pages')).toBeVisible();
  await expect(box.getByText('Private excerpt.', { exact: true })).toBeVisible();
  await box.getByRole('button', { name: 'Read original' }).click();
  await expect(
    box.getByText('Private excerpt. And the exact remaining selected text.'),
  ).toBeVisible();
  await prompt.fill('draft placeholder');
  await prompt.evaluate((input: HTMLTextAreaElement) => input.setSelectionRange(3, 7));
  await box.getByRole('button', { name: 'Remove Image #1' }).click();
  await expect(box.getByText('Image #1', { exact: true })).toHaveCount(0);
  await expect(prompt).toBeFocused();
  expect(
    await prompt.evaluate((input: HTMLTextAreaElement) => [
      input.selectionStart,
      input.selectionEnd,
    ]),
  ).toEqual([3, 7]);
  await prompt.fill('');
  await prompt.dispatchEvent('keydown', { key: 'Enter', isComposing: true, keyCode: 229 });
  await prompt.dispatchEvent('keyup', { key: 'Enter' });
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
  await page.evaluate(() => {
    window.askFixture.rejectNext = true;
  });
  await prompt.press('Enter');
  await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-1001');
  await expect(page.locator('[data-slot="prompt-box-placeholder"]')).toHaveText('Something went wrong · ASK-1001');
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(prompt).toBeFocused();
  await expect(box.getByText('PDF · 3 pages')).toBeVisible();
  await page.getByRole('button', { name: 'Send, hold to think harder' }).click();
  await expect(page.locator('.user-bubble')).toContainText('a notes.pdf');
  const requests = await page.evaluate(() => window.askFixture.submits);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
  expect(requests[0]).toMatchObject({ text: '', attachmentIds: ['pdf', 'selection'] });
});

test('DOM image drop validates files before IPC and never sends or loads a path', async ({
  page,
}) => {
  for (const invalid of ['not_image', 'oversize']) {
    const prevented = await page.locator('.composer').evaluate((composer, invalid) => {
      const dataTransfer = new DataTransfer();
      const bytes =
        invalid === 'oversize' ? new Blob([new Uint8Array(48 * 1024 * 1024 + 1)]) : 'Text';
      dataTransfer.items.add(new File([bytes], 'invalid.png', { type: 'image/png' }));
      return !composer.dispatchEvent(
        new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }),
      );
    }, invalid);
    expect(prevented).toBe(true);
    await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-2003');
    await expect(page.locator('[data-slot="prompt-box-placeholder"]')).toHaveText('Something went wrong · ASK-2003');
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(page.getByRole('textbox', { name: 'Ask anything' })).toBeFocused();
  }
  const expected = await page.locator('.composer').evaluate((composer) => {
    const dataBase64 = window.askFixture.imageBase64;
    const bytes = Uint8Array.from(atob(dataBase64), (character) => character.charCodeAt(0));
    const dataTransfer = new DataTransfer();
    dataTransfer.items.add(new File([bytes], 'dropped.png', { type: 'image/png' }));
    composer.dispatchEvent(
      new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }),
    );
    return { conversationId: 'conversation-1', displayName: 'dropped.png', dataBase64 };
  });
  await expect(
    page.locator('[data-slot="prompt-box"]').getByRole('img', { name: 'dropped.png' }),
  ).toBeVisible();
  expect(await page.evaluate(() => window.askFixture.drops)).toEqual([expected]);
  expect(await page.evaluate(() => window.askFixture.submits)).toEqual([]);
});

test('attachment expansion keeps the bottom editor usable with a bounded viewport', async ({
  page,
}) => {
  await page.setViewportSize({ width: 370, height: 242 });
  await page.evaluate(() => window.askFixture.addInitialCards());
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await expect(prompt).toBeFocused();
  await expect(prompt).toBeInViewport();
  await expect(page.getByRole('button', { name: 'Attach a file' })).toBeInViewport();
});

test('model tool status is visible without query controls; replay and sources work', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await expect(page.getByRole('button', { name: 'Web Search', exact: true })).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Public search query' })).toHaveCount(0);
  const thinking = page.getByRole('button', { name: 'Think deeply' });
  await thinking.hover();
  await thinking.click();
  await expect(thinking).toHaveAttribute('aria-pressed', 'true');
  const accent = await page.evaluate(() => {
    const sample = document.createElement('span');
    sample.style.color = 'var(--ol-blue)';
    document.body.append(sample);
    const color = getComputedStyle(sample).color;
    sample.remove();
    return color;
  });
  await expect(thinking).toHaveCSS('color', accent);
  await prompt.hover();
  await expect(thinking).toHaveCSS('color', accent);
  await prompt.fill('Search for current information');
  await prompt.press('Enter');
  await expect(page.getByRole('region', { name: 'Ask result' })).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.submits[0].options)).toEqual({
    deepThinking: true,
  });
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.truncate = true;
    const turnId = fixture.active();
    fixture.push({ type: 'searchChanged', turnId, status: 'searching', sources: [] }, false, false);
    fixture.push({
      type: 'searchChanged',
      turnId,
      status: 'reading',
      sources: [
        {
          sourceId: 'source-1',
          url: 'https://example.com/result',
          title: 'Verified result',
          readStatus: 'reading',
        },
      ],
    });
  });
  await expect.poll(() => page.evaluate(() => window.askFixture.snapshotReads)).toBeGreaterThan(1);
  await expect(page.getByText('Websearch · Reading sources…')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Sources · 1' })).toHaveCount(0);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'sourceChanged', turnId: fixture.active(), source: {
      sourceId: 'source-1', url: 'https://example.com/result', title: 'Verified result', readStatus: 'read',
    } });
  });
  const sources = page.getByRole('button', { name: 'Sources · 1' });
  await sources.click();
  await expect(page.getByRole('region', { name: 'Web search sources' })).toBeVisible();
  await page.getByRole('button', { name: /Verified result/ }).click();
  expect(await page.evaluate(() => window.askFixture.opened)).toEqual([
    ['conversation-1', 'turn-1', 'source-1'],
  ]);
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(page.getByText('Stopped · Incomplete')).toBeVisible();
  expect(await page.evaluate(() => window.askFixture.cancelled)).toEqual([
    ['conversation-1', 'turn-1'],
  ]);
  await prompt.press('Escape');
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  await page.evaluate(() => window.askFixture.activate());
  await expect(prompt).toBeFocused();
  await expect(sources).toHaveCount(0);
  await prompt.fill('Another question');
  await prompt.press('Enter');
  expect(await page.evaluate(() => window.askFixture.submits[1].options)).toEqual({
    deepThinking: true,
  });
});

test('large image drops are compressed before reaching Ask IPC', async ({ page }) => {
  await page.locator('.composer').evaluate(async (composer) => {
    const canvas = document.createElement('canvas');
    canvas.width = 3000;
    canvas.height = 4100;
    const context = canvas.getContext('2d')!;
    context.fillStyle = '#3367b4';
    context.fillRect(0, 0, canvas.width, canvas.height);
    const png = await new Promise<Blob>((resolve) => canvas.toBlob((image) => resolve(image!)));
    const dataTransfer = new DataTransfer();
    dataTransfer.items.add(new File([png], 'large.png', { type: 'image/png' }));
    composer.dispatchEvent(
      new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }),
    );
  });
  await expect.poll(() => page.evaluate(() => window.askFixture.drops.length)).toBe(1);
  const prepared = await page.evaluate(async () => {
    const image = window.askFixture.drops[0];
    const blob = await fetch(`data:image/png;base64,${image.dataBase64}`).then((result) =>
      result.blob(),
    );
    const bitmap = await createImageBitmap(blob);
    const size = [bitmap.width, bitmap.height];
    bitmap.close();
    return size;
  });
  expect(prepared).toEqual([1499, 2048]);
});

test('a rejected typed draft is restored with its raw text, selection, and request identity', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.evaluate(() => {
    window.askFixture.rejectNext = true;
  });
  await prompt.fill('  private draft  ');
  await prompt.evaluate((input: HTMLTextAreaElement) => input.setSelectionRange(2, 9, 'backward'));
  await prompt.press('Enter');
  await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-1001');
  await expect(page.locator('[data-slot="prompt-box"] [role="alert"]')).toHaveCount(0);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(prompt).toBeFocused();
  await expect(prompt).toHaveValue('  private draft  ');
  expect(
    await prompt.evaluate((input: HTMLTextAreaElement) => [
      input.selectionStart,
      input.selectionEnd,
      input.selectionDirection,
    ]),
  ).toEqual([2, 9, 'backward']);
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble')).toHaveText('private draft');
  const requests = await page.evaluate(() => window.askFixture.submits);
  expect(requests[1]).toEqual(requests[0]);
});

test('a compact draft error is visible in the input area and real editing restores the preserved draft', async ({ page }, info) => {
  await page.setViewportSize({ width: 370, height: 172 });
  await page.emulateMedia({ colorScheme: 'light' });
  const prompt = page.locator(promptInput);
  const box = page.locator('[data-slot="prompt-box"]');
  const attach = page.getByRole('button', { name: 'Attach a file' });
  await prompt.fill('Keep this draft');
  const before = (await box.boundingBox())!;
  const controlsBefore = (await attach.boundingBox())!;
  await page.evaluate(() => { window.askFixture.rejectNext = true; });
  await prompt.press('Enter');
  const error = page.locator('.ask-draft-error');
  await expect(error).toHaveText('Something went wrong · ASK-1001');
  await expect(error).toBeVisible();
  await expect(error).toBeInViewport();
  await expect(prompt).toHaveValue('Keep this draft');
  await expect(prompt).toBeFocused();
  await expect(prompt).toHaveCSS('opacity', '0');
  expect(await box.evaluate((node) => node.getAnimations().some((animation) =>
    animation.effect instanceof KeyframeEffect && animation.effect.target === node &&
    animation.effect.getKeyframes().some((frame) => String(frame.transform).includes('translateX')),
  ))).toBe(true);
  expect(await prompt.evaluate((node) => node.getAnimations().some((animation) =>
    animation.effect instanceof KeyframeEffect &&
    animation.effect.getKeyframes().some((frame) => String(frame.transform).includes('translateX')),
  ))).toBe(false);
  await expect(box).toHaveCSS('transform', 'none');
  await prompt.evaluate((input: HTMLTextAreaElement) => input.focus());
  await expect(error).toBeVisible();
  const errorBounds = (await error.boundingBox())!;
  const inputBounds = (await prompt.boundingBox())!;
  expect(errorBounds.y).toBeCloseTo(inputBounds.y);
  expect(errorBounds.x).toBeCloseTo(inputBounds.x);
  expect(errorBounds.y + errorBounds.height).toBeLessThanOrEqual(controlsBefore.y);
  expect((await box.boundingBox())!.height).toBeCloseTo(before.height);
  expect((await attach.boundingBox())!.y).toBeCloseTo(controlsBefore.y);
  await page.screenshot({ path: info.outputPath('compact-draft-error.png') });
  await prompt.press('!');
  await expect(error).toHaveCount(0);
  await expect(prompt).toHaveCSS('opacity', '1');
  await expect(prompt).toHaveValue('Keep this draft!');
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
  await prompt.press('Enter');
  await expect.poll(() => page.evaluate(() => window.askFixture.submits.length)).toBe(2);
  expect(await page.evaluate(() => window.askFixture.submits[1].text)).toBe('Keep this draft!');
});

test('compact rewrite uses only the placeholder and restores it on cancellation', async ({ page }) => {
  const prompt = page.locator(promptInput);
  const composer = page.locator('.composer');
  const placeholder = page.locator('[data-slot="prompt-box-placeholder"]');
  await page.evaluate(() => {
    window.askFixture.route = 'edit';
    localStorage.setItem('ol.locale', 'zh-CN');
    window.dispatchEvent(new StorageEvent('storage', { key: 'ol.locale' }));
  });
  await prompt.fill('Make this shorter');
  await prompt.press('Enter');
  await expect.poll(() => page.evaluate(() => window.askFixture.active())).toBeTruthy();
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'turnStatus', turnId: fixture.active(), status: 'polishing' });
  });
  await expect(composer).toHaveAttribute('data-polishing', 'true');
  await expect(placeholder).toHaveText('正在润色');
  await expect(page.getByRole('region', { name: 'Ask result' })).toHaveCount(0);
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  await prompt.fill('My next draft');
  await expect(composer).toHaveAttribute('data-polishing', 'false');
  await expect(prompt).toHaveValue('My next draft');
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'turnFinished', turnId: fixture.active(), status: 'cancelled' });
  });
  await prompt.fill('');
  await expect(placeholder).toHaveText('问任何问题');
});

test('voice claims only the original scoped editor and inserts without submitting or replacing text', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await prompt.fill('one old three');
  await prompt.evaluate((input: HTMLTextAreaElement) => input.setSelectionRange(4, 7));
  await expect.poll(() => page.evaluate(() => window.askFixture.voiceSelection())).toEqual([4, 7]);
  const bounds = await prompt.boundingBox();
  await page.evaluate(() => window.askFixture.voice({ type: 'started', recordingId: 'normal' }));
  expect(await prompt.boundingBox()).toEqual(bounds);
  await expect(prompt).toHaveValue('one old three');
  await page.evaluate(() =>
    window.askFixture.voice({
      type: 'transcribed',
      recordingId: 'normal',
      text: 'new',
    }),
  );
  await expect(prompt).toHaveValue('one oldnew three');
  expect(await prompt.evaluate((input: HTMLTextAreaElement) => input.selectionStart)).toBe(10);
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
  await page.evaluate(() => {
    window.askFixture.activate();
    window.askFixture.voice({ type: 'started', recordingId: 'combo' });
  });
  await page.evaluate(() => {
    document.querySelector('textarea')!.setSelectionRange(0, 0);
    window.askFixture.voice({
      type: 'transcribed',
      recordingId: 'combo',
      text: 'stale scope',
    });
  });
  await expect(prompt).toHaveValue('one oldnew three');
  await page.evaluate(() => {
    window.askFixture.voice({ type: 'started', recordingId: 'late' });
    window.askFixture.switch();
  });
  await expect(prompt).toHaveValue('');
  await page.evaluate(() =>
    window.askFixture.voice({
      type: 'transcribed',
      recordingId: 'late',
      text: 'not for the new conversation',
    }),
  );
  await expect(prompt).toHaveValue('');
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
});

test('only a current voice failure shakes the compact window without covering the draft', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  const composer = page.locator('[data-slot="prompt-box"]');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect.poll(() => page.evaluate(() => window.askFixture.voiceSelection())).not.toBeNull();
  await page.evaluate(() => {
    window.askFixture.voice({ type: 'started', recordingId: 'old-startup' });
    window.askFixture.freshOpen();
  });
  await expect(prompt).toBeFocused();
  await prompt.fill('Fresh draft sentinel');
  await expect.poll(() => page.evaluate(() => window.askFixture.voiceSelection())).toEqual([20, 20]);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.voice({ type: 'failed', recordingId: 'old-startup', stage: 'start' });
    fixture.voice({ type: 'started', recordingId: 'cancelled-recording' });
    fixture.voice({ type: 'cancelled', recordingId: 'cancelled-recording' });
  });
  await expect(composer.getByRole('alert')).toHaveCount(0);
  await expect(prompt).toHaveValue('Fresh draft sentinel');
  for (const [stage, scheme] of [
    ['start', 'light'],
    ['processing', 'dark'],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.evaluate((stage) => {
      const fixture = window.askFixture;
      fixture.voice({ type: 'started', recordingId: stage });
      fixture.voice({ type: 'failed', recordingId: stage, stage });
      fixture.voice({ type: 'failed', recordingId: stage, stage });
    }, stage);
    await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-2002');
    await expect(composer.getByRole('alert')).toHaveCount(0);
    await expect(prompt).toHaveValue('Fresh draft sentinel');
    await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  }
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
});

test('write, edit, and clarify have no reader side effect and retain delivery outside the draft', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.evaluate(() => {
    window.askFixture.route = 'write';
  });
  await prompt.fill('Write a short quote');
  await prompt.press('Enter');
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({ type: 'searchChanged', turnId, status: 'searching' });
    fixture.push({
      type: 'sourceChanged',
      turnId,
      source: {
        sourceId: 'price',
        url: 'https://example.com/price',
        title: 'Market price',
        readStatus: 'read',
      },
    });
    fixture.push({ type: 'searchChanged', turnId, status: 'done' });
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'write',
        status: 'writing',
        searchStatus: 'done',
        sources: [
          {
            sourceId: 'price',
            url: 'https://example.com/price',
            title: 'Market price',
            readStatus: 'read',
          },
        ],
        delivery: 'retained',
        resultText: 'Total: $420',
        copyStatus: 'failed',
      },
    });
    fixture.push({ type: 'turnFinished', turnId, status: 'completed' });
  });
  const retained = page.getByRole('textbox', { name: 'Retained result' });
  await expect(retained).toHaveValue('Total: $420');
  await retained.fill('Total: $420 USD');
  await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-2008');
  await expect(prompt).toHaveValue('Write a short quote');
  await expect(page.getByRole('button', { name: 'Sources · 1' })).toHaveCount(0);
  await retained.press('Escape');
  await page.evaluate(() => window.askFixture.activate());
  await expect(retained).toHaveValue('Total: $420 USD');
  await page.evaluate(() => {
    window.askFixture.route = 'edit';
  });
  await prompt.fill('Make it shorter');
  await expect(page.getByRole('button', { name: 'Send, hold to think harder' })).toBeEnabled();
  await prompt.press('Enter');
  await expect(retained).toHaveCount(0);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'edit',
        status: 'writing',
        searchStatus: 'idle',
        sources: [],
        delivery: 'written',
        copyStatus: 'notNeeded',
      },
    });
    fixture.push({ type: 'turnFinished', turnId, status: 'completed' });
    fixture.route = 'clarify';
  });
  await expect(page.getByText('Replaced in the original editor.')).toBeVisible();
  await prompt.fill('Fix it');
  await prompt.press('Enter');
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'clarify',
        status: 'thinking',
        searchStatus: 'idle',
        sources: [],
        resultText: 'Select what you want to change.',
        copyStatus: 'notNeeded',
      },
    });
    fixture.push({ type: 'turnFinished', turnId, status: 'completed' });
  });
  await expect(page.getByText('Select what you want to change.')).toBeVisible();
  await expect(page.locator('.assistant-text')).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.expandCount)).toBe(0);
});

test('an unconfirmed edit preserves the result and sent instruction without overwriting a newer draft or retrying', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.evaluate(() => { window.askFixture.route = 'edit'; });
  await prompt.fill('Make it shorter');
  await prompt.press('Enter');
  await expect.poll(() => page.evaluate(() => window.askFixture.active())).toBeTruthy();
  await prompt.fill('A newer draft');
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'edit',
        status: 'writing',
        searchStatus: 'idle',
        sources: [],
        delivery: 'unknown',
        resultText: 'Candidate shortened text',
        copyStatus: 'notNeeded',
      },
    });
    fixture.push({ type: 'turnFinished', turnId, status: 'completed' });
  });
  const composer = page.locator('[data-slot="prompt-box"]');
  await expect(prompt).toHaveValue('A newer draft');
  await expect(composer.getByRole('textbox', { name: 'Retained result' })).toHaveValue('Candidate shortened text');
  await expect(composer.getByText('Check the original editor before pasting.', { exact: false })).toBeVisible();
  const previous = composer.locator('details').filter({ hasText: 'Previous instruction' });
  await previous.locator('summary').click();
  await expect(previous.getByText('Make it shorter', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
});

test('a failed delivery review keeps the original instruction, reference, and copyable result in compact Ask', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 300 });
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  const composer = page.locator('[data-slot="prompt-box"]');
  await page.evaluate(() => {
    window.askFixture.route = 'write';
    window.askFixture.addInitialCards();
  });
  await prompt.fill('Write an email');
  await prompt.press('Enter');
  await expect.poll(() => page.evaluate(() => window.askFixture.active())).toBeTruthy();
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'write',
        status: 'writing',
        searchStatus: 'idle',
        sources: [],
        delivery: 'retained',
        resultText: 'Subject: [Subject]\n\nHello,\n\nMay we meet on [date]?',
        copyStatus: 'notNeeded',
      },
    });
    fixture.push({
      type: 'turnFinished',
      turnId,
      status: 'failed',
      error: { code: 'provider', message: 'The generated text could not be reviewed and was not written.', reason: 'askDraftReviewUnavailable' },
    });
  });
  await expect(prompt).toHaveValue('Write an email');
  const copyable = composer.getByRole('textbox', { name: 'Retained result' });
  const alert = composer.getByRole('alert');
  await expect(copyable).toHaveValue(/May we meet/);
  await expect(alert).toHaveCount(0);
  await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-1103');
  await expect(copyable).toBeInViewport();
  await expect(prompt).toBeInViewport();
  await expect(composer.locator('.attachment-card')).toHaveCount(3);
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
});

test('expanded errors use the shared top popup and retain generated text without review UI', async ({ page }) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  const composer = page.locator('[data-slot="prompt-box"]');
  await page.evaluate(() => window.askFixture.addInitialCards());
  await prompt.fill('Describe the attached image');
  await prompt.press('Enter');
  await expect(page.locator('.ask-expansion-backdrop')).toBeVisible();
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'answerDelta', turnId: fixture.active(), messageId: 'approved-first', text: 'An approved first detail.' });
  });
  await expect(page.locator('.assistant-text')).toHaveText('An approved first detail.');
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({
      type: 'delivery',
      submission: {
        currentTurnId: turnId,
        route: 'answer',
        status: 'thinking',
        searchStatus: 'idle',
        sources: [],
        resultText: 'An approved first detail.\n\nUnreviewed tail sentinel.',
        copyStatus: 'notNeeded',
      },
    });
    fixture.push({
      type: 'turnFinished',
      turnId,
      status: 'failed',
      error: { code: 'provider', message: 'askAnswerReviewUnavailable' },
    });
  });
  await expect(page.locator('.assistant-text')).toHaveText('An approved first detail.');
  await expect(composer.getByRole('alert')).toHaveCount(0);
  await expect(page.locator('.ol-notification-center').getByRole('alert')).toHaveText('Something went wrong · ASK-1103');
  await expect(page.locator('.transcript-surface > [role="alert"]')).toHaveCount(0);
  await expect(prompt).toHaveValue('Describe the attached image');
  await expect(composer.locator('.attachment-card')).toHaveCount(3);
  await expect(composer.getByRole('textbox', { name: 'Unreviewed response' })).toHaveCount(0);
  await expect(composer.getByText('Show unreviewed response', { exact: true })).toHaveCount(0);
  await expect(composer.getByRole('textbox', { name: 'Retained result' })).toHaveValue(/Unreviewed tail sentinel/);
  await page.evaluate(() => window.askFixture.freshOpen());
  await expect(prompt).toHaveValue('');
  await expect(page.getByRole('textbox', { name: 'Unreviewed response' })).toHaveCount(0);
  await expect(page.locator('.assistant-text')).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
});

test('expanded voice follow-up waits for Send and a fresh opening clears the old chat', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('Opening A sentinel');
  await prompt.press('Enter');
  await expect(page.locator('.ask-expansion-backdrop')).toBeVisible();
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'turnFinished', turnId: fixture.active(), status: 'completed' });
    fixture.route = 'edit';
  });
  await expect(prompt).toBeFocused();
  await expect.poll(() => page.evaluate(() => window.askFixture.voiceSelection())).toEqual([0, 0]);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.voice({ type: 'started', recordingId: 'chat-recording' });
    fixture.voice({ type: 'transcribed', recordingId: 'chat-recording', text: 'Make this shorter' });
  });
  await expect(prompt).toHaveValue('Make this shorter');
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble').last()).toHaveText('Make this shorter');
  expect(await page.evaluate(() => window.askFixture.expandCount)).toBe(1);
  await prompt.press('Escape');
  await page.evaluate(() => window.askFixture.freshOpen());
  await expect(page.locator('.user-bubble')).toHaveCount(0);
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  await expect(prompt).toHaveValue('');
  await expect(prompt).toBeFocused();
});

test('expanded scroll follows growth only while pinned and jump returns to the composer', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('Give me a long answer');
  await prompt.press('Enter');
  const thread = page.locator('.ask-thread');
  await expect(thread).toBeVisible();
  const bottom = () => thread.evaluate((element) => element.scrollHeight - element.clientHeight - element.scrollTop);
  const delta = async () => page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({ type: 'answerDelta', turnId: fixture.active(), messageId: 'long-answer',
      text: 'Disposable paragraph for scrolling.\n\n'.repeat(90) });
  });
  await delta();
  await expect.poll(bottom).toBeLessThan(25);
  await thread.evaluate((element) => { element.scrollTop = 0; });
  const jump = page.getByRole('button', { name: 'Jump to latest' });
  await expect(jump).toBeVisible();
  const position = await thread.evaluate((element) => element.scrollTop);
  await delta();
  await expect.poll(() => thread.evaluate((element) => element.scrollTop)).toBe(position);
  await jump.focus();
  await jump.press('Enter');
  await expect(jump).toHaveCount(0);
  await expect(prompt).toBeFocused();
  await expect.poll(bottom).toBeLessThan(25);
});

for (const [locale, message] of [
  ['zh-CN', '出现了一点错误 · ASK-0000'],
  ['en-US', 'Something went wrong · ASK-0000'],
  ['de-DE', 'Ein Fehler ist aufgetreten · ASK-0000'],
]) {
  test(`compact errors preserve geometry, clear on input and expire (${locale})`, async ({ page }, info) => {
    await page.setViewportSize({ width: 370, height: 172 });
    await page.emulateMedia({ colorScheme: 'light' });
    await page.evaluate((locale) => {
      localStorage.setItem('ol.locale', locale);
      window.dispatchEvent(new Event('languagechange'));
    }, locale);
    const prompt = page.locator(promptInput);
    const box = page.locator('[data-slot="prompt-box"]');
    const bounds = (await box.boundingBox())!;
    await page.evaluate(() => window.askFixture.notice({ code: 'unrecognized-secret', message: 'RAW_BACKEND_SECRET', reason: 'PRIVATE_REASON' }));
    const placeholder = page.locator('[data-slot="prompt-box-placeholder"]');
    await expect(placeholder).toHaveText(message);
    const animationTarget = await box.evaluate((node) => node.getAnimations().some((a) => a.effect instanceof KeyframeEffect && a.effect.target === node));
    expect(animationTarget).toBe(true);
    await expect(box).toHaveCSS('transform', 'none');
    const after = (await box.boundingBox())!;
    expect(after.height).toBeCloseTo(bounds.height);
    expect(after.y).toBeCloseTo(bounds.y);
    expect(await placeholder.locator('.truncate').evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await expect(page.locator('.ol-notification-center [role="alert"]')).toHaveCount(0);
    await expect(page.getByText('RAW_BACKEND_SECRET', { exact: false })).toHaveCount(0);
    await page.screenshot({ path: info.outputPath(`compact-${locale}.png`) });
    await prompt.fill('Draft stays exactly here');
    await expect(page.locator('.composer')).not.toHaveAttribute('data-error-code');
    const draftBounds = (await box.boundingBox())!;
    await page.evaluate(() => window.askFixture.notice({ code: 'unknown', message: 'Never render this' }));
    await expect(page.locator('.composer')).toHaveAttribute('data-error-code', 'ASK-0000');
    await expect(prompt).toHaveValue('Draft stays exactly here');
    await expect(placeholder).toHaveCount(0);
    await expect(page.locator('.ask-draft-error')).toBeVisible();
    await expect(page.locator('.ask-draft-error')).toHaveText(message);
    await expect(box).toHaveCSS('transform', 'none');
    expect((await box.boundingBox())!.height).toBeCloseTo(draftBounds.height);
    await expect(page.locator('.composer')).not.toHaveAttribute('data-error-code', { timeout: 6000 });
    await expect(prompt).toHaveValue('Draft stays exactly here');
    await page.evaluate(() => window.askFixture.notice({ code: 'unknown', message: 'Never render this' }));
    await expect(page.locator('.ask-draft-error')).toBeVisible();
    await prompt.press('End');
    await expect(page.locator('.ask-draft-error')).toHaveCount(0);
    await prompt.press('!');
    await expect(prompt).toHaveValue('Draft stays exactly here!');
    expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
    await prompt.fill('');
    await expect(placeholder).not.toContainText('ASK-');
  });
}

test('Escape cancels the active turn and replay cannot revive it or apply a foreign request', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('First request');
  await prompt.press('Enter');
  await expect(page.locator('.ask-presentation')).toHaveAttribute('data-expanded', 'true');
  await prompt.fill('Next draft');
  await prompt.press('Escape');
  await expect.poll(() => page.evaluate(() => window.askFixture.cancelled.length)).toBe(1);
  await page.evaluate(() => {
    const f = window.askFixture;
    f.push({ type: 'answerDelta', turnId: 'turn-1', messageId: 'late', text: 'LATE_CANCELLED' });
    f.push({ type: 'turnFinished', turnId: 'turn-1', status: 'failed', error: { code: 'provider', message: 'LATE_ERROR' } });
    f.activate();
  });
  await expect(prompt).toHaveValue('Next draft');
  await expect(page.locator('.composer')).not.toHaveAttribute('data-error-code');
  await expect(page.getByRole('button', { name: 'Send, hold to think harder' })).toBeEnabled();
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble').last()).toHaveText('Next draft');
  await page.evaluate(() => {
    const f = window.askFixture;
    f.push({ type: 'answerDelta', turnId: f.active(), clientRequestId: 'wrong-request', messageId: 'wrong', text: 'FOREIGN_REQUEST' });
    f.push({ type: 'answerDelta', turnId: f.active(), clientRequestId: f.submits.at(-1)!.clientRequestId, messageId: 'correct', text: 'Correct answer' });
  });
  await expect(page.locator('.assistant-text')).toHaveText('Correct answer');
  await expect(page.getByText(/LATE_CANCELLED|LATE_ERROR|FOREIGN_REQUEST/)).toHaveCount(0);
});

test('Escape during submit cancels its eventual receipt without expanding or losing the draft', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await page.evaluate(() => { window.askFixture.deferSubmit = true; });
  await prompt.fill('Pending draft');
  await prompt.press('Enter');
  await expect(prompt).toHaveAttribute('readonly');
  await prompt.press('Escape');
  await page.evaluate(() => window.askFixture.releaseSubmit());
  await expect.poll(() => page.evaluate(() => window.askFixture.cancelled)).toEqual([['conversation-1', 'turn-1']]);
  await page.evaluate(() => window.askFixture.activate());
  await expect(prompt).toHaveValue('Pending draft');
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
});

test('an expanded error never moves the editor and clears before a new request', async ({ page }, info) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('Start a question');
  await prompt.press('Enter');
  await expect(page.locator('.ask-presentation')).toHaveAttribute('data-expanded', 'true');
  await page.evaluate(() => window.askFixture.push({ type: 'answerDelta', turnId: window.askFixture.active(), messageId: 'partial', text: 'Keep this partial answer.' }));
  await prompt.fill('A newer instruction');
  const box = page.locator('[data-slot="prompt-box"]');
  const bounds = (await box.boundingBox())!;
  await page.evaluate(() => window.askFixture.notice({ code: 'unexpected', message: 'SECRET_PROVIDER_TITLE' }));
  const popup = page.locator('.ol-notification-center').getByRole('alert');
  await expect(popup).toHaveText('Something went wrong · ASK-0000');
  await expect(popup).toBeInViewport();
  expect((await popup.boundingBox())!.y).toBeLessThan(100);
  expect((await box.boundingBox())!.height).toBeCloseTo(bounds.height);
  expect((await box.boundingBox())!.y).toBeCloseTo(bounds.y);
  await expect(page.locator('.ask-inline-notice')).toHaveCount(0);
  await expect(page.getByText('Failed · Incomplete')).toHaveCount(0);
  await expect(page.locator('.window-bar')).not.toContainText('Ask');
  await expect(page.getByText('SECRET_PROVIDER_TITLE')).toHaveCount(0);
  await expect(prompt).toHaveValue('A newer instruction');
  await page.screenshot({ path: info.outputPath('expanded-error.png') });
  await prompt.fill('Changed instruction');
  await expect(popup).toHaveCount(0);
  await page.evaluate(() => window.askFixture.push({ type: 'turnFinished', turnId: window.askFixture.active(), status: 'completed' }));
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble').last()).toHaveText('Changed instruction');
  await expect(popup).toHaveCount(0);
});

test('general tools replay independently, expose real statuses and cannot restart after cancellation', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('Check my context');
  await prompt.press('Enter');
  await expect(page.locator('.ask-presentation')).toHaveAttribute('data-expanded', 'true');
  await page.evaluate(() => {
    const f = window.askFixture;
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'time', name: 'current_time', status: 'running' }, false);
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'history', name: 'voice_history', status: 'running' });
  });
  const time = page.locator('[data-tool-call-id="time"]');
  const history = page.locator('[data-tool-call-id="history"]');
  await expect(time).toContainText('current_timeRunning');
  await expect(history).toHaveAttribute('data-status', 'running');
  await page.evaluate(() => {
    const f = window.askFixture;
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'time', name: 'current_time', status: 'completed' });
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'time', name: 'current_time', status: 'running' });
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'history', name: 'voice_history', status: 'failed', error: { code: 'unknown', message: 'PRIVATE_TOOL_ERROR' } });
  });
  await expect(time).toHaveAttribute('data-status', 'completed');
  await expect(history).toHaveAttribute('data-status', 'failed');
  await expect(page.locator('.ol-notification-center').getByRole('alert')).toHaveText('Something went wrong · ASK-0000');
  await expect(page.getByText('PRIVATE_TOOL_ERROR')).toHaveCount(0);
  await page.evaluate(() => {
    const f = window.askFixture;
    f.push({ type: 'toolChanged', turnId: f.active(), toolCallId: 'system', name: 'system_context', status: 'running' });
  });
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect(page.locator('[data-tool-call-id="system"]')).toHaveAttribute('data-status', 'cancelled');
  await page.evaluate(() => window.askFixture.push({ type: 'toolChanged', turnId: 'turn-1', toolCallId: 'system', name: 'system_context', status: 'completed' }));
  await expect(page.locator('[data-tool-call-id="system"]')).toHaveAttribute('data-status', 'cancelled');
});

test('a previous failed turn cannot contaminate the next submitted request', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('First question');
  await prompt.press('Enter');
  await expect(page.locator('.ask-presentation')).toHaveAttribute('data-expanded', 'true');
  await page.evaluate(() => window.askFixture.push({ type: 'turnFinished', turnId: window.askFixture.active(), status: 'failed', error: { code: 'provider', message: 'OLD_FAILURE' } }));
  const popup = page.locator('.ol-notification-center').getByRole('alert');
  await expect(popup).toContainText('ASK-0000');
  await prompt.fill('Next question');
  await expect(popup).toHaveCount(0);
  await page.evaluate(() => window.askFixture.push({ type: 'submitted', submission: {
    currentTurnId: 'next-unresolved', clientRequestId: 'next-request', status: 'queued', searchStatus: 'idle', sources: [], copyStatus: 'notNeeded',
  } }));
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeEnabled();
  await expect(popup).toHaveCount(0);
  await expect(prompt).toHaveValue('Next question');
});

test('a late delivery after cancellation cannot replace the retained content', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await page.evaluate(() => { window.askFixture.route = 'write'; });
  await prompt.fill('Write a note');
  await prompt.press('Enter');
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Stop', exact: true }).click();
  await page.evaluate(() => window.askFixture.push({ type: 'delivery', submission: {
    currentTurnId: 'turn-1', status: 'completed', route: 'write', searchStatus: 'idle', sources: [], copyStatus: 'notNeeded', resultText: 'LATE_DELIVERY', delivery: 'written',
  } }));
  await expect(prompt).toHaveValue('Write a note');
  await expect(page.getByRole('textbox', { name: 'Retained result' })).toHaveCount(0);
  await expect(page.getByText('Inserted in the original editor.')).toHaveCount(0);
});

test('a rejected pending request restores its draft after Escape without showing an old error', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await page.evaluate(() => { window.askFixture.deferSubmit = true; window.askFixture.rejectNext = true; });
  await prompt.fill('Keep this rejected draft');
  await prompt.press('Enter');
  await expect(prompt).toHaveAttribute('readonly');
  await prompt.press('Escape');
  await page.evaluate(() => window.askFixture.releaseSubmit());
  await expect(prompt).toHaveValue('Keep this rejected draft');
  await page.evaluate(() => window.askFixture.activate());
  await expect(page.getByRole('button', { name: 'Send, hold to think harder' })).toBeEnabled();
  await expect(page.locator('.composer')).not.toHaveAttribute('data-error-code');
});

test('a completed request replaces streamed working text with its authoritative final answer', async ({ page }) => {
  const prompt = page.locator(promptInput);
  await prompt.fill('Explain this');
  await prompt.press('Enter');
  await page.evaluate(() => {
    const f = window.askFixture;
    const turnId = f.active();
    f.push({ type: 'answerDelta', turnId, messageId: 'answer', text: 'Incomplete working text' });
    f.push({ type: 'turnFinished', turnId, status: 'completed', text: 'The complete final answer.' });
    f.push({ type: 'answerDelta', turnId, messageId: 'answer', text: 'LATE_DELTA' });
  });
  await expect(page.locator('.assistant-text')).toHaveText('The complete final answer.');
  await expect(page.getByText(/Incomplete working text|LATE_DELTA/)).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Stop', exact: true })).toHaveCount(0);
});
