import { expect, test, type Page } from '@playwright/test';
const fixture = 'http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html';
async function patch(page: Page, value: Record<string, unknown>) {
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate((value) => (window as any).fixture.setCapsule(value), value);
}
async function emit(page: Page, event: string, payload: unknown) {
  await page.evaluate(({ event, payload }) => (window as any).fixture.emit(event, payload), {
    event,
    payload,
  });
}
const messages = [
  { role: 'user', content: '这是他询问任何问题的内容' },
  {
    role: 'assistant',
    content:
      '这句话的意思是：这是他用来提问任何问题的具体内容。\n\n如果你有任何问题或需要帮助，请随时告诉我，我会尽力为你解答。',
  },
];

test('the new capsule keeps a frosted, readable surface in both themes', async ({ page }, info) => {
  await page.goto(fixture);
  const pill = page.locator('.ol-default-capsule');
  await expect(pill).toHaveCSS('opacity', '1');
  await expect(pill).toHaveCSS('filter', 'none');
  for (const theme of ['light', 'dark']) {
    await page.evaluate(
      (theme) => document.documentElement.setAttribute('data-ol-theme', theme),
      theme,
    );
    await patch(page, { state: 'recording', translation: true });
    await expect(pill).toHaveCSS('width', '116px');
    const material = await pill.evaluate((element) => {
      const background = getComputedStyle(element, '::before');
      const channels = background.backgroundColor.match(/[\d.]+/g)!.map(Number);
      return {
        luminance: (channels[0] + channels[1] + channels[2]) / (3 * 255),
        alpha: channels[3],
        blur: background.backdropFilter || background.getPropertyValue('-webkit-backdrop-filter'),
        scheme: getComputedStyle(element).colorScheme,
      };
    });
    expect(material.scheme).toBe(theme);
    expect(material.alpha).toBeGreaterThan(0.8);
    expect(material.alpha).toBeLessThan(1);
    expect(material.blur).toContain('blur(');
    if (theme === 'dark') expect(material.luminance).toBeLessThan(0.25);
    else expect(material.luminance).toBeGreaterThan(0.85);
    await expect(pill.getByRole('button', { name: '取消', exact: true })).toBeEnabled();
    await expect(page.locator('.ol-classic-translation')).toHaveCSS('color-scheme', theme);
    await page.screenshot({ path: info.outputPath(`capsule-${theme}.png`) });
    await patch(page, { state: 'polishing' });
    await expect(pill.locator('[role="status"]')).toContainText('thinking');
    await expect(pill).toHaveCSS('opacity', '1');
  }
});

test('capsule contracts continuously, changes content in sequence, and can reverse mid-flight', async ({
  page,
}) => {
  await page.goto(fixture);
  const pill = page.locator('.ol-default-capsule');
  await expect(pill).toHaveCSS('width', '116px');
  await expect(pill).toHaveCSS('height', '32px');
  await expect(pill.locator('.ol-default-waveform > span')).toHaveCount(11);
  const samples = await page.evaluate(async () => {
    const pill = document.querySelector('.ol-default-capsule')!;
    (window as any).fixture.setCapsule({ state: 'polishing' });
    const frames: number[] = [];
    const start = performance.now();
    while (performance.now() - start < 300) {
      await new Promise(requestAnimationFrame);
      frames.push(pill.getBoundingClientRect().width);
    }
    return frames;
  });
  expect(samples.some((width) => width > 81 && width < 114)).toBeTruthy();
  await expect(pill).toHaveCSS('width', '80px');
  await expect(pill.locator('.ol-default-status')).toHaveCSS('opacity', '1');
  expect(await pill.evaluate((el) => getComputedStyle(el, '::after').animationName)).toBe(
    'ol-capsule-sweep',
  );
  await patch(page, { state: 'recording' });
  await page.waitForTimeout(60);
  await patch(page, { state: 'polishing' });
  await page.waitForTimeout(50);
  await patch(page, { state: 'recording' });
  await expect(pill).toHaveCSS('width', '116px');
  await patch(page, { leaving: true });
  await page.waitForTimeout(60);
  expect(await pill.evaluate((element) => getComputedStyle(element).filter)).toContain('blur(');
  await patch(page, { leaving: false });
  await expect(pill).toHaveCSS('opacity', '1');
  await expect(pill).toHaveCSS('filter', 'none');
  await expect(pill).toHaveCSS('width', '116px');
});

test('translation strip settles above the pill and exits when processing starts', async ({
  page,
}) => {
  await page.goto(fixture);
  await patch(page, { translation: true });
  const badge = page.locator('.ol-classic-translation');
  await expect(badge).toHaveCSS('opacity', '1');
  const body = await page.locator('.ol-default-capsule').boundingBox();
  const strip = await badge.boundingBox();
  expect(body!.y - strip!.y - strip!.height).toBeCloseTo(8, 0);
  await patch(page, { state: 'polishing' });
  await expect(badge).toHaveCSS('opacity', '0');
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('width', '80px');
});

test('translation target popup stays interactive during a recording session', async ({ page }) => {
  await page.goto(fixture);
  await patch(page, { state: 'recording', translation: true });
  const badge = page.locator('.ol-classic-translation');
  const trigger = badge.getByRole('combobox', { name: '翻译目标语言' });
  await expect(trigger).toBeEnabled();
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  const japanese = page.getByRole('option').filter({ hasText: /日语|日本語/ }).first();
  await expect(japanese).toBeVisible();
  await japanese.click();
  await expect(trigger).toContainText(/日语|日本語/);
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.targetChanges.length))
    .toBe(1);
  await expect
    .poll(() => page.evaluate(() => (window as any).fixture.menuChanges.at(-1)?.open))
    .toBe(false);
});

test('QA records in a pill, reveals a wide result, retains content on exit and reopens', async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await expect(page.locator('.qa-prompt')).toHaveCount(0);
  await emit(page, 'qa:state', { kind: 'recording', sessionId: 'a', messages: [] });
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await expect(page.locator('.qa-prompt')).toBeHidden();
  await emit(page, 'qa:state', { kind: 'thinking', sessionId: 'a', messages: [messages[0]] });
  await expect(page.locator('.qa-answer-panel')).toHaveCount(0);
  await expect(page.locator('.ol-default-capsule')).toHaveAttribute('data-processing', 'true');
  await emit(page, 'qa:state', { kind: 'answer', sessionId: 'a', messages });
  await expect(page.locator('#native-host')).toHaveCSS('width', '760px');
  await expect(page.locator('.qa-answer-panel')).toBeVisible();
  await expect(page.locator('.qa-capsule-shell')).toHaveCSS('opacity', '1');
  await page.screenshot({ path: info.outputPath('qa-result.png') });
  await emit(page, 'chat-panel:closing', {});
  await expect(page.locator('.qa-assistant-message')).toContainText('我会尽力为你解答');
  await page.waitForTimeout(60);
  expect(
    await page.locator('.qa-capsule-shell').evaluate((el) => Number(getComputedStyle(el).opacity)),
  ).toBeLessThan(1);
  await emit(page, 'chat-panel:shown', {});
  await emit(page, 'qa:state', { kind: 'recording', sessionId: 'b', messages: [] });
  await expect(page.locator('.qa-capsule-shell')).not.toHaveClass(/is-closing/);
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  await expect(page.locator('#native-host')).toHaveCSS('height', '80px');
  expect(errors).toEqual([]);
});

test('immediate geometry acknowledgements keep each QA result visible', async ({ page }) => {
  await page.goto(fixture + '?qa');
  await expect(page.locator('.ol-default-capsule')).toBeVisible();
  for (let index = 0; index < 12; index++) {
    const sessionId = `instant-layout-${index}`;
    await emit(page, 'qa:state', { kind: 'recording', sessionId, messages: [] });
    await expect(page.locator('.ol-default-capsule')).toBeVisible();
    await expect(page.locator('#native-host')).toHaveCSS('width', '480px');
    await emit(page, 'qa:state', { kind: 'answer', sessionId, messages });
    await expect(page.locator('.qa-answer-panel')).toBeVisible();
    await expect(page.locator('#native-host')).toHaveCSS('width', '760px');
  }
});

test('reduced motion disables sweeps and geometry interpolation', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto(fixture);
  await patch(page, { state: 'polishing', translation: true });
  await expect(page.locator('.ol-default-capsule')).toHaveCSS('width', '80px');
  expect(
    await page
      .locator('.ol-default-capsule')
      .evaluate((el) => getComputedStyle(el, '::after').animationName),
  ).toBe('none');
});

test('native idle keeps the same capsule node through exit; a new recording cancels removal', async ({
  page,
}) => {
  await page.goto(fixture + '?full');
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await emit(page, 'capsule:state', { state: 'recording', level: 0.2, capsuleStyle: 'classic' });
  const pill = page.locator('.ol-default-capsule');
  await expect(pill).toHaveCSS('width', '116px');
  await pill.evaluate((el) => {
    (window as any).originalPill = el;
  });
  const retained = await page.evaluate(async () => {
    (window as any).fixture.emit('capsule:state', { state: 'idle' });
    await new Promise((resolve) => setTimeout(resolve, 50));
    const same = document.querySelector('.ol-default-capsule') === (window as any).originalPill;
    (window as any).fixture.emit('capsule:state', { state: 'recording', level: 0.1 });
    return same;
  });
  expect(retained).toBe(true);
  await expect(pill).toHaveCSS('opacity', '1');
  await page.waitForTimeout(250);
  await expect(pill).toBeVisible();
  await emit(page, 'capsule:state', { state: 'idle' });
  await expect(pill).toHaveCount(0);
});
