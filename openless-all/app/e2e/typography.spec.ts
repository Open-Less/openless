import { expect, test, type Page } from '@playwright/test';

test.use({ baseURL: 'http://127.0.0.1:5326' });
const errors = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const messages: string[] = [];
  errors.set(page, messages);
  page.on('pageerror', (error) => messages.push(error.message));
  await page.goto('/e2e/fixtures/typography.html');
  await expect(page.getByRole('button', { name: 'Render', exact: true })).toBeVisible();
});
test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

async function render(page: Page, text: string, action = 'Render') {
  await page.getByRole('textbox', { name: 'Markdown' }).fill(text);
  await page.getByRole('button', { name: action, exact: true }).click();
}

// Read computed styles at 0/45/90ms, not just the keyframes the implementation requested.
async function captureNextReveal(page: Page, hold = false) {
  await page.evaluate((hold) => {
    const probe = window as Window & { reveal?: unknown };
    delete probe.reveal;
    const observer = new MutationObserver(() => {
      const spans = [...document.querySelectorAll<HTMLElement>('.olchat-reveal-glyph')];
      if (!spans.length) return;
      observer.disconnect();
      const timings = spans.map((span) => {
        const animation = span.getAnimations()[0];
        return {
          text: span.textContent,
          duration: animation.effect!.getTiming().duration,
          age: animation.currentTime,
        };
      });
      const animation = spans[0].getAnimations()[0];
      const age = animation.currentTime;
      animation.pause();
      const frames = [0, 45, 90].map((time) => {
        animation.currentTime = time;
        const style = getComputedStyle(spans[0]);
        return {
          opacity: Number(style.opacity),
          blur: Number(style.filter.match(/[\d.]+/)?.[0] ?? 0),
        };
      });
      animation.currentTime = age;
      if (hold) spans.forEach((span) => span.getAnimations()[0].pause());
      else animation.play();
      probe.reveal = { timings, frames };
    });
    observer.observe(document.querySelector('#answer')!, { subtree: true, childList: true });
  }, hold);
}

async function readReveal(page: Page) {
  await expect
    .poll(() => page.evaluate(() => Boolean((window as Window & { reveal?: unknown }).reveal)))
    .toBe(true);
  return page.evaluate(
    () =>
      (
        window as Window & {
          reveal: {
            timings: { text: string; duration: number; age: number }[];
            frames: { opacity: number; blur: number }[];
          };
        }
      ).reveal,
  );
}

test('theme weights follow the actual surface and return unchanged in light mode', async ({
  page,
}) => {
  await expect(page.locator('#medium')).toHaveCSS('font-weight', '500');
  await expect(page.locator('#semibold')).toHaveCSS('font-weight', '600');
  await page.getByRole('button', { name: 'Dark', exact: true }).click();
  await expect(page.locator('#medium')).toHaveCSS('font-weight', '460');
  await expect(page.locator('#semibold')).toHaveCSS('font-weight', '560');
  await expect(page.locator('#light-medium')).toHaveCSS('font-weight', '500');
  await render(page, '### 标题\n\n**重点正文**');
  await expect(page.locator('.olchat-answer h3')).toHaveCSS('font-weight', '560');
  for (const strong of await page.locator('.olchat-answer strong').all()) {
    await expect(strong).toHaveCSS('font-weight', '560');
  }
  await page.getByRole('button', { name: 'Light', exact: true }).click();
  await expect(page.locator('#medium')).toHaveCSS('font-weight', '500');
  for (const strong of await page.locator('.olchat-answer strong').all()) {
    await expect(strong).toHaveCSS('font-weight', '600');
  }
});

test('CJK and Latin tails stay together at narrow widths, even without text-wrap pretty', async ({
  page,
}) => {
  await render(
    page,
    '这段中文正文不应该让最后一个字单独占行。\n\nA short paragraph should keep its final words.',
  );
  const answer = page.locator('.olchat-answer');
  // Simulate engines without `pretty`: the structural fallback must do the work itself.
  await answer
    .locator('p')
    .evaluateAll((paragraphs) =>
      paragraphs.forEach((p) => (p as HTMLElement).style.setProperty('text-wrap', 'wrap')),
    );
  for (const width of [130, 169, 208, 260, 335, 640]) {
    await page.locator('#answer').evaluate((element, size) => {
      (element as HTMLElement).style.width = `${size}px`;
    }, width);
    const tails = await answer.locator('.ol-prose-tail').evaluateAll((elements) =>
      elements.map((element) => {
        const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        const tops: number[] = [];
        while (walker.nextNode()) {
          const text = walker.currentNode as Text;
          for (let i = 0; i < text.length; i++) {
            if (/\s/.test(text.data[i])) continue;
            const range = document.createRange();
            range.setStart(text, i);
            range.setEnd(text, i + 1);
            tops.push(range.getBoundingClientRect().top);
          }
        }
        return { text: element.textContent, spread: Math.max(...tops) - Math.min(...tops) };
      }),
    );
    expect(tails.map((tail) => tail.text)).toEqual(['占行。', 'final words.']);
    expect(tails.every((tail) => tail.spread < 1)).toBe(true);
    expect(
      await answer.evaluate((element) => element.scrollWidth - element.clientWidth),
    ).toBeLessThanOrEqual(1);
  }
});

test('wrapping preserves copy text, inline semantics, hard breaks, lists and code', async ({
  page,
}) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await render(
    page,
    '前面的文字，最后**两个字**。\n\nRead the [final link](https://example.com?a=1&b=2).\n\nFirst line\n末行\n\n- 列表最后两字\n\n`inline_code`\n\n```js\nconst longIdentifier = "unchanged source with spaces";\n```',
  );
  const answer = page.locator('.olchat-answer');
  expect(await answer.locator('p').nth(0).textContent()).toBe('前面的文字，最后两个字。');
  expect(await answer.locator('p').nth(0).locator('strong').allTextContents()).toEqual([
    '两',
    '个字',
  ]);
  await expect(answer.locator('a')).toHaveCount(1);
  await expect(answer.locator('a')).toHaveAttribute('href', 'https://example.com?a=1&b=2');
  await expect(answer.locator('a')).toHaveAttribute('rel', 'noreferrer noopener');
  await expect(answer.locator('br')).toHaveCount(1);
  await expect(answer.locator('li .ol-prose-tail')).toHaveText('两字');
  await expect(answer.locator('pre .ol-prose-tail, code .ol-prose-tail')).toHaveCount(0);
  expect(await answer.locator('pre code').textContent()).toBe(
    'const longIdentifier = "unchanged source with spaces";\n',
  );
  expect(await answer.textContent()).not.toMatch(/[\u00a0\u2060]/u);
  await expect(answer.locator('pre')).toHaveCSS('text-wrap-mode', 'nowrap');
  await render(page, 'Before ' + 'long'.repeat(70) + ' final.');
  expect(
    await answer.evaluate((element) => element.scrollWidth - element.clientWidth),
  ).toBeLessThanOrEqual(1);
  await render(page, '<img src=x onerror=alert(1)> [bad](javascript:alert(1))');
  await expect(answer.locator('img, script, [onerror], [href^="javascript:"]')).toHaveCount(0);
});

test('new graphemes reveal in 90ms, stagger, settle, and never replay old text', async ({
  page,
}) => {
  await captureNextReveal(page);
  await render(page, '你好 👨‍👩‍👧‍👦 e\u0301', 'Stream');
  const first = await readReveal(page);
  expect(first.timings.map((entry) => entry.text)).toEqual(['你', '好', '👨‍👩‍👧‍👦', 'e\u0301']);
  expect(first.timings.every((entry) => entry.duration === 90)).toBe(true);
  expect(first.timings[0].age).toBeGreaterThan(first.timings[1].age);
  expect(first.frames[0]).toEqual({ opacity: 0, blur: 2 });
  expect(first.frames[1].opacity).toBeGreaterThan(0);
  expect(first.frames[1].opacity).toBeLessThan(1);
  expect(first.frames[1].blur).toBeGreaterThan(0);
  expect(first.frames[1].blur).toBeLessThan(2);
  expect(first.frames[2]).toEqual({ opacity: 1, blur: 0 });
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await captureNextReveal(page);
  await render(page, '你好 👨‍👩‍👧‍👦 e\u0301 新内容', 'Stream');
  expect((await readReveal(page)).timings.map((entry) => entry.text).join('')).toBe('新内容');
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await expect(page.locator('.olchat-answer')).toHaveText('你好 👨‍👩‍👧‍👦 e\u0301 新内容');
});

test('changing motion preferences and finishing during a reveal flushes all text', async ({
  page,
}) => {
  await captureNextReveal(page, true);
  await render(page, '正在逐字显示', 'Stream');
  await readReveal(page);
  await expect(page.locator('.olchat-reveal-glyph')).not.toHaveCount(0);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await expect(page.locator('.olchat-answer')).toHaveText('正在逐字显示');
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await captureNextReveal(page, true);
  await render(page, '正在逐字显示最后一段', 'Stream');
  await readReveal(page);
  await render(page, '正在逐字显示最后一段。完整结果。', 'Finish');
  await expect(page.locator('.olchat-answer')).toHaveText('正在逐字显示最后一段。完整结果。');
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await expect(page.locator('.olchat-caret')).toHaveCount(0);
});

test('Markdown rewrites do not replay prose and streaming code stays plain', async ({ page }) => {
  await render(page, '开头 **加粗', 'Stream');
  await expect(page.locator('.olchat-answer')).toHaveText('开头 **加粗');
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await render(page, '开头 **加粗文字**', 'Stream');
  await expect(page.locator('.olchat-answer')).toHaveText('开头 加粗文字');
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await page.getByRole('button', { name: 'Unmount', exact: true }).click();
  await render(page, '```js\nconst code = "not animated";\n```', 'Stream');
  await expect(page.locator('.olchat-answer pre code')).toHaveText('const code = "not animated";');
  await expect(page.locator('pre .olchat-reveal-glyph')).toHaveCount(0);
});

test('burst catch-up is bounded; completion, reduced motion and unmount release animations', async ({
  page,
}) => {
  await captureNextReveal(page);
  await render(page, '长回答'.repeat(2000), 'Stream');
  const burst = await readReveal(page);
  expect(burst.timings.length).toBeLessThanOrEqual(96);
  expect(burst.timings.every((entry) => entry.age >= -90)).toBe(true);
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await expect(page.locator('.olchat-answer')).toHaveText('长回答'.repeat(2000));
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await render(page, '减少动态效果：直接显示。', 'Stream');
  await expect(page.locator('.olchat-answer')).toHaveText('减少动态效果：直接显示。');
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await render(page, '停止时也不能丢掉最后一个字。', 'Finish');
  await expect(page.locator('.olchat-answer')).toHaveText('停止时也不能丢掉最后一个字。');
  await expect(page.locator('.olchat-caret')).toHaveCount(0);
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await expect(page.locator('.olchat-reveal-glyph')).toHaveCount(0);
  await render(page, '停止时也不能丢掉最后一个字。追加', 'Stream');
  await page.getByRole('button', { name: 'Unmount', exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        document.getAnimations().filter((animation) => animation.id === 'ol-answer-reveal').length,
    ),
  ).toBe(0);
});
