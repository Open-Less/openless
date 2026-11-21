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
    fixture.push({ type: 'turnFinished', turnId: fixture.active(), status: 'completed' });
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
  await expect(box.getByRole('alert')).toHaveText('Set up an Ask model first.');
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
    const warning = page.locator('[data-slot="prompt-box"]').getByRole('alert');
    await expect(warning).toContainText(
      invalid === 'oversize'
        ? 'This image is too large to prepare safely.'
        : 'Drop a valid PNG or JPEG image.',
    );
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
  const errorNotice = page.locator('[data-slot="prompt-box"]').getByRole('alert');
  await expect(errorNotice).toHaveText('Set up an Ask model first.');
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

test('only a current voice failure appears inside the composer; a previous opening and cancellation are silent', async ({
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
  for (const [stage, message, scheme] of [
    ['start', 'Could not start voice input.', 'light'],
    ['processing', 'Voice input did not finish.', 'dark'],
  ] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.evaluate((stage) => {
      const fixture = window.askFixture;
      fixture.voice({ type: 'started', recordingId: stage });
      fixture.voice({ type: 'failed', recordingId: stage, stage });
      fixture.voice({ type: 'failed', recordingId: stage, stage });
    }, stage);
    const notice = composer.getByText(message, { exact: false });
    await expect(notice).toHaveCount(1);
    await expect(notice).toBeVisible();
    const noticeBounds = (await notice.boundingBox())!;
    expect(noticeBounds.y + noticeBounds.height).toBeLessThanOrEqual((await prompt.boundingBox())!.y);
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
  await expect(page.getByText('Could not copy to the clipboard.')).toBeVisible();
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
  await expect(composer.getByText('Ask could not confirm whether the editor changed.', { exact: false })).toBeVisible();
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
  await expect(alert).toHaveCount(1);
  await expect(alert).toContainText('Ask could not review the generated text');
  await expect(alert).toBeInViewport();
  await expect(copyable).toBeInViewport();
  await expect(prompt).toBeInViewport();
  await expect(composer.locator('.attachment-card')).toHaveCount(3);
  await expect(page.locator('.ask-expansion-backdrop')).toHaveCount(0);
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(1);
});

test('an expanded attached question retains a failed review as a scoped unreviewed copy, not a chat answer', async ({ page }) => {
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
  await expect(composer.getByRole('alert')).toHaveCount(1);
  await expect(composer.getByRole('alert')).toContainText('Ask could not review the complete answer.');
  await expect(prompt).toHaveValue('Describe the attached image');
  await expect(composer.locator('.attachment-card')).toHaveCount(3);
  await expect(composer.getByRole('textbox', { name: 'Unreviewed response' })).toHaveCount(0);
  await composer.getByText('Show unreviewed response', { exact: true }).click();
  await expect(composer.getByRole('textbox', { name: 'Unreviewed response' })).toHaveValue(/Unreviewed tail sentinel/);
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
