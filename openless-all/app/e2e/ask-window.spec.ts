import { expect, test } from '@playwright/test';
import type {} from './fixtures/ask-window';
import { fixtureOrigin } from './support/server';

test.use({ viewport: { width: 800, height: 920 }, locale: 'en-US' });

test.beforeEach(async ({ page }) => {
  await page.goto(`${fixtureOrigin}/e2e/fixtures/ask-window.html`);
  await expect(page.getByRole('textbox', { name: 'Ask anything' })).toBeFocused();
});

test('first send moves the original composer and restores real state after gaps and hide', async ({
  page,
}) => {
  await expect(page.locator('#ask-root')).toHaveCSS('border-radius', '24px');
  const prompt = page.getByRole('textbox', { name: 'Ask anything' });
  const box = page.locator('[data-slot="prompt-box"]');
  expect((await box.boundingBox())!.y).toBeLessThan(500);
  await prompt.fill('hi');
  await prompt.press('Enter');
  await expect(page.locator('.user-bubble')).toHaveText('hi');
  await expect(page.locator('.effort-label')).toHaveCount(0);
  await expect(page.getByText('Working', { exact: true })).toBeVisible();
  await expect.poll(async () => (await box.boundingBox())!.y).toBeGreaterThan(750);
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
  const errorNotice = page.locator('.ol-toast-notification-card[data-type="error"]');
  await expect(errorNotice.getByRole('alert')).toHaveText('Set up an Ask model first.');
  await expect(errorNotice.locator('[data-tone="error"]')).toHaveCSS('width', '36px');
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
    const warning = page.locator('.ol-toast-notification-card[data-type="warning"]');
    await expect(warning.locator('[data-tone="warning"]')).toHaveCSS('width', '36px');
    await expect(warning.getByRole('alert')).toContainText(
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
  await expect(page.getByText('Working', { exact: true })).toBeVisible();
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
  const errorNotice = page.locator('.ol-toast-notification-card[data-type="error"]');
  await expect(errorNotice.getByRole('alert')).toHaveText('Set up an Ask model first.');
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
  await expect(page.locator('.ol-toast-notification-card[data-type="error"]')).toContainText(
    'Set up an Ask model first.',
  );
  await expect(page.getByRole('dialog')).toHaveCount(0);
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
