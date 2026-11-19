import { expect, test } from '@playwright/test';
import type {} from './fixtures/ask-window';
import { fixtureOrigin } from './support/server';

test.use({ viewport: { width: 920, height: 800 }, locale: 'en-US' });

test.beforeEach(async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/ask-window.html`);
  await expect(page.getByRole('textbox', { name: 'Ask anything' })).toBeFocused();
});

test('first send moves the original composer and restores real state after gaps and hide', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  const box = page.locator('[data-slot="prompt-box"]');
  expect((await box.boundingBox())!.y).toBeLessThan(440);
  await prompt.fill('hi');
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble')).toHaveText('hi');
  await expect(page.getByText('Working', { exact: true })).toBeVisible();
  await expect.poll(async () => (await box.boundingBox())!.y).toBeGreaterThan(630);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    const turnId = fixture.active();
    fixture.push({ type: 'turnStatus', turnId, status: 'thinking' }, false);
    fixture.push({ type: 'answerDelta', turnId, messageId: 'answer', text: 'Hello' }, false);
    fixture.push({ type: 'answerDelta', turnId, messageId: 'answer', text: ' there.' });
  });
  await expect(page.locator('.assistant-text')).toHaveText('Hello there.');
  await prompt.fill('keep this draft');
  await page.getByRole('button', { name: 'Close Ask Anything' }).click();
  await expect.poll(() => page.evaluate(() => window.askFixture.liveListeners())).toBe(0);
  await page.evaluate(() => {
    const fixture = window.askFixture;
    fixture.push({
      type: 'turnFinished',
      turnId: fixture.active(),
      status: 'failed',
      error: { code: 'provider', message: 'Disconnected.' },
    });
    fixture.activate();
  });
  await expect(page.getByText('Failed · Incomplete')).toBeVisible();
  await expect(page.locator('.assistant-text')).toHaveText('Hello there.');
  await expect(prompt).toHaveValue('keep this draft');
  expect(
    await page.evaluate(() => [window.askFixture.hideCount, window.askFixture.submits.length]),
  ).toEqual([1, 1]);
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
  await expect(page.getByRole('alert')).toHaveText('Set up an Ask model first.');
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
      const bytes = invalid === 'oversize' ? new Uint8Array(8 * 1024 * 1024 + 1) : 'Text';
      dataTransfer.items.add(new File([bytes], 'invalid.png', { type: 'image/png' }));
      return !composer.dispatchEvent(
        new DragEvent('drop', { dataTransfer, bubbles: true, cancelable: true }),
      );
    }, invalid);
    expect(prevented).toBe(true);
    await expect(page.getByRole('alert')).toHaveText(
      invalid === 'oversize' ? 'Images must be 8 MiB or smaller.' : 'Drop a PNG or JPEG image.',
    );
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

test('explicit public query and actual search status; truncation uses a snapshot and sources open natively', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await page.getByRole('button', { name: 'Web Search', exact: true }).click();
  const query = page.getByRole('textbox', { name: 'Public search query', exact: true });
  await query.fill('public words');
  await query.press('Enter');
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
  await page.getByRole('button', { name: 'Confirm public query' }).click();
  await page.getByRole('button', { name: 'Think deeply' }).click();
  await prompt.fill('private draft');
  await prompt.press('Enter');
  await expect(page.getByText('Working', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.askFixture.submits[0].options)).toEqual({
    deepThinking: true,
    webSearch: true,
    publicSearchQuery: 'public words',
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
  await prompt.fill('do not reuse the prior approval');
  await prompt.press('Enter');
  expect(await page.evaluate(() => window.askFixture.submits[1].options)).toEqual({
    deepThinking: true,
    webSearch: true,
  });
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
  await expect(page.getByRole('alert')).toHaveText('Set up an Ask model first.');
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

test('normal voice preserves input geometry and selection, combo retries without overwriting draft', async ({
  page,
}) => {
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  await prompt.fill('one old three');
  await prompt.evaluate((input: HTMLTextAreaElement) => input.setSelectionRange(4, 7));
  const bounds = await prompt.boundingBox();
  await page.evaluate(() =>
    window.askFixture.voice({ type: 'started', mode: 'insert', recordingId: 'normal' }),
  );
  expect(await prompt.boundingBox()).toEqual(bounds);
  await expect(prompt).toHaveValue('one old three');
  await page.evaluate(() =>
    window.askFixture.voice({
      type: 'transcribed',
      mode: 'insert',
      recordingId: 'normal',
      text: 'new',
    }),
  );
  await expect(prompt).toHaveValue('one new three');
  expect(await prompt.evaluate((input: HTMLTextAreaElement) => input.selectionStart)).toBe(7);
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(0);
  await page.evaluate(() => {
    window.askFixture.rejectNext = true;
    window.askFixture.voice({ type: 'started', mode: 'submit', recordingId: 'combo' });
    window.askFixture.voice({
      type: 'transcribed',
      mode: 'submit',
      recordingId: 'combo',
      text: 'spoken separately',
    });
  });
  await expect(page.getByRole('button', { name: 'Retry recording' })).toBeEnabled();
  await expect(prompt).toHaveValue('one new three');
  await page.getByRole('button', { name: 'Retry recording' }).click();
  await expect(page.locator('.user-bubble')).toHaveText('spoken separately');
  await expect(prompt).toHaveValue('one new three');
  expect(
    await page.evaluate(() => {
      const [failed, retried] = window.askFixture.submits;
      return failed.clientRequestId === retried.clientRequestId;
    }),
  ).toBe(true);
  await page.evaluate(() => {
    window.askFixture.voice({ type: 'started', mode: 'submit', recordingId: 'late' });
    window.askFixture.switch();
  });
  await expect(prompt).toHaveValue('');
  await page.evaluate(() =>
    window.askFixture.voice({
      type: 'transcribed',
      mode: 'submit',
      recordingId: 'late',
      text: 'not for the new conversation',
    }),
  );
  expect(await page.evaluate(() => window.askFixture.submits.length)).toBe(2);
});
