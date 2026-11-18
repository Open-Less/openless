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
  await expect(page.locator('input')).toHaveCount(0);
  await emit(page, 'qa:state', {
    kind: 'recording',
    sessionId: 'ask-a',
    messages: [],
    selectionPreview: 'selected source',
  });
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(page.locator('.qa-recording-selection')).toContainText('selected source');
  await expect(page.locator('#native-host')).toHaveCSS('width', '975px');
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
  await expect(panel).toHaveCSS('border-radius', '0px');
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
  expect((await draft.boundingBox())!.height).toBeGreaterThan(24);

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
  await expect(panel).toHaveCSS('width', '975px');
  await expect(panel).toHaveCSS('height', '415px');
  const question = { role: 'user', content: 'What are the sources?' };
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'search', messages: [question] });
  const response = panel.locator('.qa-assistant-message');
  await response.evaluate((element) => element.setAttribute('data-retained', 'yes'));
  const body = 'Some streaming text.\n\n'.repeat(28) + '[An inline source](https://example.org/a)';
  await emit(page, 'qa:state', { kind: 'answer_delta', sessionId: 'search', chunk: body });
  const viewport = panel.locator('[data-slot=message-scroller-viewport]');
  await expect
    .poll(() => viewport.evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop))
    .toBeLessThan(2);
  await expect(panel.locator('textarea')).toBeInViewport();
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
        content: `${body}\n\n[1](<https://example.org/a>) [2](<https://example.com/b>)`,
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
