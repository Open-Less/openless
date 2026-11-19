import { fixtureOrigin } from './support/server';
import { expect, test, type Page } from '@playwright/test';
const fixture = `${fixtureOrigin}/e2e/fixtures/capsule-motion.html`;
async function emit(page: Page, event: string, payload: unknown) {
  await page.evaluate((v) => (window as any).fixture.emit(v.event, v.payload), { event, payload });
}

test('translation uses the shared popup and native headroom; save failure preserves the old language', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 460, height: 172 });
  await page.exposeFunction('resizeMenuForTest', async (open: boolean) => {
    await page.setViewportSize({ width: 460, height: open ? 492 : 172 });
  });
  await page.goto(fixture);
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate(() => (window as any).fixture.setCapsule({ translation: true }));
  const picker = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(picker).toBeEnabled();
  await picker.click();
  const popup = page.locator('.ol-select-popup');
  await expect(popup).toBeVisible();
  await expect(popup).toHaveClass(/ol-capsule-language-popup/);
  await expect(popup).toHaveCSS('backdrop-filter', 'none');
  await popup.evaluate((node) => node.setAttribute('data-test-retained', 'true'));
  for (let i = 0; i < 8; i += 1) {
    await page.evaluate((level) => (window as any).fixture.setCapsule({ level }), i / 10);
    await expect(popup).toHaveAttribute('data-test-retained', 'true');
    await expect(popup).toHaveCSS('opacity', '1');
  }
  const clipped = await popup.evaluate((node) =>
    getComputedStyle(node).clipPath.startsWith('shape('),
  );
  await expect(popup).toHaveCSS('border-radius', clipped ? '0px' : '16px');
  const bounds = await popup.boundingBox();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.height).toBeGreaterThan(200);
  await page.screenshot({ path: info.outputPath('translation-menu.png') });
  await page.getByRole('option', { name: '日语', exact: true }).click();
  await expect(picker).toContainText('日语');
  await expect(popup).toBeHidden();
  await expect.poll(() => page.viewportSize()?.height).toBe(172);
  expect(await page.evaluate(() => (window as any).fixture.targetChanges.at(-1))).toEqual({
    sessionId: 'recording-a',
    targetLanguage: '日本語',
  });
  await page.evaluate(() => ((window as any).fixture.failTarget = true));
  await picker.click();
  await page.getByRole('option', { name: '法语', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('语言切换失败');
  await expect(picker).toContainText('日语');
  await expect(picker).toBeEnabled();
});

test('translation popup closes when the recording owner changes', async ({ page }) => {
  await page.goto(fixture);
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate(() => (window as any).fixture.setCapsule({ translation: true }));
  const picker = page.getByRole('combobox', { name: '翻译目标语言' });
  await expect(picker).toBeEnabled();
  await picker.click();
  await expect(page.locator('.ol-select-popup')).toBeVisible();
  await page.evaluate(() => {
    const f = (window as any).fixture;
    f.target.sessionId = 'recording-b';
    f.emit('capsule:state', { state: 'recording', sessionId: 'recording-b', translation: true });
  });
  await expect(page.locator('.ol-select-popup')).toBeHidden();
  expect(await page.evaluate(() => (window as any).fixture.targetChanges)).toEqual([]);
});

test('Ask opens a conversation on record, streams activity, and keeps a composer', async ({
  page,
}, info) => {
  await page.goto(fixture + '?qa');
  await page.locator('.ol-default-capsule').waitFor();
  await expect(page.locator('.qa-prompt textarea')).toHaveCount(0);
  await emit(page, 'qa:state', {
    kind: 'recording',
    sessionId: 'ask-a',
    messages: [],
    selectionPreview: 'selected source',
  });
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(page.locator('.qa-recording-selection')).toContainText('selected source');
  await expect(page.locator('#native-host')).toHaveCSS('width', '920px');
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'ask-a', messages: [] });
  await emit(page, 'qa:state', {
    kind: 'activity',
    sessionId: 'ask-a',
    activity: { kind: 'searching_web', toolName: 'web_search' },
  });
  await expect(page.locator('.qa-thought')).toContainText('正在搜索网页');
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'ask-a', chunk: 'partial' });
  await expect(page.locator('.qa-assistant-message')).toContainText('partial');
  await emit(page, 'qa:state', {
    kind: 'activity',
    sessionId: 'old-ask',
    activity: { kind: 'using_tool', toolName: 'stale' },
  });
  await expect(page.locator('.qa-thought')).not.toContainText('stale');
  await emit(page, 'qa:state', {
    kind: 'answer',
    sessionId: 'ask-a',
    messages: [
      { role: 'user', content: '问题', selectionText: 'selected source' },
      { role: 'assistant', content: '最终回答' },
    ],
  });
  await expect(page.locator('.qa-user-turn')).toContainText('问题');
  await expect(page.locator('.qa-user-turn')).toContainText('selected source');
  await expect(page.locator('.qa-assistant-message')).toContainText('最终回答');
  await expect(page.locator('.qa-thought')).toContainText('已思考');
  await expect(page.locator('.qa-follow-up')).toHaveCount(0);
  await expect(page.locator('.qa-prompt textarea')).toBeVisible();
  await expect(page.locator('.qa-prompt .qa-close')).toHaveCount(0);
  await page.screenshot({ path: info.outputPath('ask-result.png') });
});

test('Stop cancels the current turn and retains the open conversation for a follow-up', async ({
  page,
}) => {
  await page.goto(fixture + '?qa');
  await page.locator('.ol-default-capsule').waitFor();
  const firstTurn = [
    { role: 'user', content: 'first question' },
    { role: 'assistant', content: 'first answer' },
  ];
  await emit(page, 'qa:state', { kind: 'answer', sessionId: 'ask-a', messages: firstTurn });
  await expect(page.locator('.qa-assistant-message')).toContainText('first answer');
  await emit(page, 'qa:state', {
    kind: 'thinking',
    sessionId: 'ask-b',
    messages: [...firstTurn, { role: 'user', content: 'follow-up' }],
  });
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'ask-b', chunk: 'draft answer' });
  const stop = page.locator('.qa-send.is-stop');
  await expect(stop).toBeVisible();
  await expect(stop.locator('span').last()).toHaveCSS('pointer-events', 'none');
  await stop.click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).fixture.qaActions.map((action: { command: string }) => action.command),
      ),
    )
    .toContain('qa_cancel_turn');
  await emit(page, 'qa:state', { kind: 'cancelled', sessionId: 'ask-b', messages: firstTurn });
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(page.locator('.qa-assistant-message')).toContainText('first answer');
  await expect(page.getByText('draft answer')).toHaveCount(0);
  await expect(page.locator('.qa-prompt textarea')).toBeEnabled();
  await expect(stop).toHaveCount(0);
});

test('overview is restored and the Ask page remains without the added three-row guide', async ({
  page,
}, info) => {
  await page.goto('/');
  await expect(page.locator('[data-ol-page="overview"]')).toBeVisible();
  await expect(page.locator('.ol-recording-key-guide')).toHaveCount(0);
  await page.screenshot({ path: info.outputPath('overview-restored.png') });
  const nav = page.locator('.ol-sidebar-surface nav');
  await nav.getByRole('button', { name: '工具', exact: true }).click();
  await nav.getByRole('button', { name: '随便问', exact: true }).click();
  await expect(page.getByText('提问附加键', { exact: true })).toBeVisible();
  await expect(page.locator('.ol-recording-key-guide')).toHaveCount(0);
  await expect(page.getByText('保存历史', { exact: true })).toBeVisible();
});

test('the macOS prompt box preserves the draft through the voice chord and allows multiline text', async ({
  page,
}) => {
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'platform', { value: 'MacIntel' }),
  );
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  const firstTurn = [
    { role: 'user', content: 'First question' },
    { role: 'assistant', content: 'First answer' },
  ];
  await emit(page, 'qa:state', { kind: 'answer', sessionId: 'first', messages: firstTurn });
  const panel = page.locator('.qa-conversation');
  await expect(panel).toHaveCSS('border-radius', '26px');
  const close = panel.locator('.qa-close');
  await expect(close).toHaveCSS('border-radius', '50%');
  await expect(panel.locator('.qa-window-actions')).toHaveCSS('left', '10px');
  const draft = panel.locator('textarea');
  await draft.fill('typed');
  await draft.press('Space');
  await draft.press('Shift+Enter');
  await draft.press('a');
  const typed = 'typed \na';
  await expect(draft).toHaveValue(typed);
  await expect.poll(async () => (await draft.boundingBox())?.height ?? 0).toBeGreaterThan(40);

  // WKWebView does not always expose Fn; the native hook consumes the chord first.
  // When a browser reports it, the editor must also not insert a space.
  const leakedChordPrevented = await draft.evaluate((editor) => {
    const key = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    Object.defineProperty(key, 'getModifierState', { value: (name: string) => name === 'Fn' });
    return !editor.dispatchEvent(key);
  });
  expect(leakedChordPrevented).toBe(true);
  await emit(page, 'qa:state', { kind: 'recording', sessionId: 'voice', messages: firstTurn });
  await expect(draft).toHaveAttribute('readonly', '');
  await expect(draft).toHaveAttribute('placeholder', '你正在进行语音输入');
  await expect(panel.locator('.voice-waveform-label')).toHaveText('你正在进行语音输入');
  await expect(draft).toHaveValue(typed);
  await emit(page, 'qa:state', { kind: 'cancelled', sessionId: 'voice', messages: firstTurn });
  await expect(draft).toBeEditable();
  await expect(draft).toHaveValue(typed);
  await draft.press('Enter');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.qaActions.at(-1)))
    .toMatchObject({
      command: 'qa_submit_text',
      args: { text: typed, expectedSessionId: 'voice' },
    });
});

test('the Windows conversation clips and scrolls streamed text, and opens live and final source links', async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem('ol.locale', 'en');
    Object.defineProperty(navigator, 'platform', { value: 'Win32' });
  });
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'recording', sessionId: 'search', messages: [] });
  const panel = page.locator('.qa-conversation');
  const close = panel.locator('.qa-close');
  await expect(panel).toHaveAttribute('data-platform', 'win');
  await expect(panel.locator('.qa-window-actions')).toHaveCSS('right', '10px');
  await expect(close).toHaveCSS('border-radius', '50%');
  await expect(panel.locator('textarea')).toHaveAttribute(
    'placeholder',
    'You are using voice input',
  );
  await expect(panel).toHaveCSS('overflow', 'hidden');
  await expect(panel).toHaveCSS('width', '920px');
  await expect(panel).toHaveCSS('height', '800px');
  const question = { role: 'user', content: 'What are the sources?' };
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'search', messages: [question] });
  const response = panel.locator('.qa-assistant-message');
  await response.evaluate((element) => element.setAttribute('data-retained', 'yes'));
  const firstChunk = 'Some streaming text.\n\n'.repeat(28);
  const lastChunk = '\n\n[An inline source](https://example.org/a)';
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'search', chunk: firstChunk });
  const viewport = panel.locator('[data-slot=message-scroller-viewport]');
  await expect
    .poll(() => viewport.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop))
    .toBeLessThan(2);
  await expect(panel.locator('textarea')).toBeInViewport();
  await viewport.hover();
  await page.mouse.wheel(0, -420);
  const jump = panel.getByRole('button', { name: 'Jump to latest' });
  await expect(jump).toBeVisible();
  await expect(jump).toHaveAttribute('data-active', 'true');
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'search', chunk: lastChunk });
  await expect(response.getByRole('link', { name: 'An inline source' })).toBeAttached();
  expect(
    await viewport.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop),
  ).toBeGreaterThan(200);
  await jump.click();
  await expect
    .poll(() => viewport.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop))
    .toBeLessThan(2);
  await response.getByRole('link', { name: 'An inline source' }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.externalLinks))
    .toEqual(['https://example.org/a']);
  await emit(page, 'qa:state', {
    kind: 'answer',
    sessionId: 'search',
    messages: [
      question,
      {
        role: 'assistant',
        content: `${firstChunk}${lastChunk}\n\n[1](<https://example.org/a>) [2](<https://example.com/b>)`,
      },
    ],
  });
  await expect(response).toHaveAttribute('data-retained', 'yes');
  const source = response.getByRole('link', { name: '2. example.com (example.com)' });
  await source.click();
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.externalLinks))
    .toEqual(['https://example.org/a', 'https://example.com/b']);
});

test('the welcome box docks on first send without remounting the editor', async ({ page }) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'welcome', messages: [] });
  await expect(page.locator('.qa-thought')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'answer', sessionId: 'welcome', messages: [] });
  const panel = page.locator('.qa-conversation');
  const prompt = panel.locator('.qa-prompt');
  const editor = prompt.locator('textarea');
  await expect(panel.locator('.qa-welcome-title')).toBeVisible();
  await expect(prompt).toHaveCSS('width', '560px');
  await expect(prompt).toHaveCSS('height', '102px');
  await expect
    .poll(async () => {
      const frame = (await panel.boundingBox())!;
      const box = (await prompt.boundingBox())!;
      return Math.abs(box.y + box.height / 2 - frame.y - frame.height / 2);
    })
    .toBeLessThan(40);
  await editor.fill(Array(10).fill('Long draft').join('\n'));
  await expect(editor).toHaveCSS('height', '192px');
  await editor.fill('A first question');
  await editor.evaluate((el) => el.setAttribute('data-original-editor', 'yes'));
  await editor.press('Enter');
  await expect.poll(() => page.evaluate(() => (window as any).fixture.qaActions.length)).toBe(1);
  await emit(page, 'qa:state', {
    kind: 'thinking',
    sessionId: 'welcome',
    messages: [{ role: 'user', content: 'A first question' }],
  });
  await expect(panel.locator('.qa-welcome-title')).toHaveCount(0);
  await expect(editor).toHaveAttribute('data-original-editor', 'yes');
  await expect(editor).toBeFocused();
  await expect(editor).toBeEditable();
  await editor.fill('Another draft');
  await expect(editor).toHaveValue('Another draft');
  await expect
    .poll(async () => {
      const frame = (await panel.boundingBox())!;
      const box = (await prompt.boundingBox())!;
      return Math.abs(frame.y + frame.height - box.y - box.height);
    })
    .toBeLessThan(30);
});

test('the waiting composer opens without a recording and voice dictation only edits its draft', async ({
  page,
}) => {
  await page.goto(fixture + '?qa&qaBehavior=draft');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'idle', panelOpen: true, messages: [] });
  const panel = page.locator('.qa-conversation');
  const editor = panel.locator('textarea');
  await expect(panel.locator('.qa-welcome-title')).toBeVisible();
  await expect(editor).toBeFocused();
  await expect(panel.locator('.qa-voice-strip')).toHaveCount(0);
  await editor.fill('Before after');
  await editor.evaluate((el: HTMLTextAreaElement) => el.setSelectionRange(7, 7));
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.qaDraftFocus?.focused))
    .toBe(true);
  await expect.poll(async () => (await panel.boundingBox())?.width).toBe(920);
  const frame = await panel.boundingBox();
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('qa:draft-recording', {
      token: bridge.qaDraftFocus.token,
      sessionId: 'dictation',
      phase: 'recording',
      level: 0.8,
      elapsedMs: 3000,
    });
  });
  const strip = panel.locator('.qa-voice-strip');
  await expect(strip).toHaveCSS('width', '340px');
  await expect(strip).toHaveCSS('height', '52px');
  await expect(strip.locator('.qa-voice-strip-control')).toHaveCSS('width', '40px');
  await expect(strip.locator('.qa-voice-strip-bars i')).toHaveCount(40);
  await expect(strip.locator('time')).toHaveText('0:03');
  await expect
    .poll(() =>
      strip
        .locator('i')
        .last()
        .evaluate((bar) => parseFloat(bar.style.height)),
    )
    .toBeGreaterThan(20);
  await expect(editor).toBeFocused();
  await expect(editor).toBeEditable();
  expect(await panel.boundingBox()).toEqual(frame);
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('qa:draft-recording', {
      token: bridge.qaDraftFocus.token,
      sessionId: 'dictation',
      phase: 'recording',
      level: 0,
      elapsedMs: 3500,
    });
  });
  await expect
    .poll(() =>
      strip
        .locator('i')
        .last()
        .evaluate((bar) => parseFloat(bar.style.height)),
    )
    .toBe(5);
  expect(
    await strip
      .locator('i')
      .evaluateAll((bars) =>
        bars.slice(0, -1).some((bar) => parseFloat((bar as HTMLElement).style.height) > 20),
      ),
  ).toBe(true);
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('qa:draft-recording', {
      token: bridge.qaDraftFocus.token,
      sessionId: 'dictation',
      phase: 'processing',
    });
  });
  await expect(strip).toHaveCount(0);
  await editor.press('Enter');
  expect(await page.evaluate(() => (window as any).fixture.qaActions)).toEqual([]);
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('qa:dictation-draft', { token: 'old-window', text: 'stale' });
    bridge.emit('qa:dictation-draft', { token: bridge.qaDraftFocus.token, text: 'spoken ' });
    bridge.emit('qa:draft-recording', {
      token: bridge.qaDraftFocus.token,
      sessionId: 'dictation',
      phase: 'idle',
    });
  });
  await expect(strip).toHaveCount(0);
  await expect(editor).toBeFocused();
  await expect(editor).toHaveValue('Before spoken after');
  await expect(panel.locator('.qa-send')).toBeEnabled();
  expect(await page.evaluate(() => (window as any).fixture.qaActions)).toEqual([]);
  await editor.press('Enter');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.qaActions.at(-1)))
    .toMatchObject({
      command: 'qa_submit_text',
      args: { text: 'Before spoken after' },
    });
});

test('draft dictation failure is shown without moving focus or losing text', async ({ page }) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'idle', panelOpen: true, messages: [] });
  const panel = page.locator('.qa-conversation');
  const editor = panel.locator('textarea');
  await editor.fill('Keep this');
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.qaDraftFocus?.focused))
    .toBe(true);
  await page.evaluate(() => {
    const bridge = (window as any).fixture;
    bridge.emit('qa:draft-recording', {
      token: bridge.qaDraftFocus.token,
      sessionId: 'failed',
      phase: 'failed',
      error: 'PermissionDenied',
    });
  });
  await expect(panel.getByRole('alert')).toBeVisible();
  await expect(editor).toHaveValue('Keep this');
  await expect(editor).toBeFocused();
  await expect(panel.locator('.qa-voice-strip')).toHaveCount(0);
});

test('typed send never clips the conversation or displays recording feedback', async ({ page }) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await emit(page, 'qa:state', { kind: 'idle', panelOpen: true, messages: [] });
  const panel = page.locator('.qa-conversation');
  const editor = panel.locator('textarea');
  await expect(page.locator('.qa-capsule-shell')).toHaveCSS('opacity', '1');
  await expect.poll(async () => (await panel.boundingBox())?.width).toBe(920);
  const frame = await panel.boundingBox();
  await editor.fill('A typed question');
  const frames = await panel.evaluate(async (el) => {
    el.querySelector('textarea')!.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    const observations = [];
    for (let frame = 0; frame < 8; frame++) {
      await new Promise<void>((done) => requestAnimationFrame(() => done()));
      observations.push({
        clip: getComputedStyle(el).clipPath,
        recording: !!el.querySelector('.qa-voice-strip'),
      });
    }
    return observations;
  });
  expect(frames.every(({ clip, recording }) => clip === 'none' && !recording)).toBe(true);
  await expect.poll(() => page.evaluate(() => (window as any).fixture.qaActions.length)).toBe(1);
  await emit(page, 'qa:state', { kind: 'loading', sessionId: 'typed', messages: [] });
  expect(await panel.boundingBox()).toEqual(frame);
  await expect(panel.locator('.qa-voice-strip')).toHaveCount(0);
  await expect(editor).toBeFocused();
  await expect(editor).toBeEditable();
});
