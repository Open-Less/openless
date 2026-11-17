import { expect, test } from '@playwright/test';

test('IME confirmation and the next 500ms do not trigger Enter handlers', async ({ page }) => {
  await page.goto('http://127.0.0.1:5326/');
  const result = await page.evaluate(async () => {
    const input = document.createElement('input');
    document.body.append(input);
    let submitted = 0;
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') submitted++;
    });
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    const confirm = new KeyboardEvent('keydown', {
      key: 'Enter',
      bubbles: true,
      cancelable: true,
      isComposing: true,
    });
    input.dispatchEvent(confirm);
    input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    const recent = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    input.dispatchEvent(recent);
    const during = submitted;
    await new Promise((resolve) => setTimeout(resolve, 520));
    input.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }),
    );
    return {
      during,
      submitted,
      imeDefaultPrevented: confirm.defaultPrevented,
      recentPrevented: recent.defaultPrevented,
    };
  });
  expect(result).toEqual({
    during: 0,
    submitted: 1,
    imeDefaultPrevented: false,
    recentPrevented: true,
  });
});

test('message follow yields to the reader and resumes at the bottom', async ({ page }) => {
  await page.goto('http://127.0.0.1:5326/');
  await page.evaluate(async () => {
    const root = document.createElement('div');
    root.id = 'follow-fixture';
    root.style.cssText = 'position:fixed;top:0;left:0;width:200px;z-index:99999';
    root.innerHTML =
      '<div data-slot="message-scroller-viewport" style="height:100px;overflow:auto"><div data-slot="message-scroller-content" style="height:300px"></div></div>';
    document.body.append(root);
    const modulePath = '/src/lib/messageFollow.ts';
    const { installMessageFollow } = await import(modulePath);
    installMessageFollow(root);
  });
  const viewport = page.locator('#follow-fixture [data-slot="message-scroller-viewport"]');
  const distance = () =>
    viewport.evaluate((node) => node.scrollHeight - node.clientHeight - node.scrollTop);
  await expect.poll(distance).toBeLessThanOrEqual(1);
  await viewport.evaluate((node) => {
    node.scrollTop -= 30;
    node.dispatchEvent(new Event('scroll'));
    (node.firstElementChild as HTMLElement).style.height = '360px';
  });
  await expect.poll(distance).toBeLessThanOrEqual(1);
  const held = await viewport.evaluate((node) => {
    node.dispatchEvent(new Event('touchmove'));
    node.scrollTop -= 30;
    node.dispatchEvent(new Event('scroll'));
    const value = node.scrollTop;
    (node.firstElementChild as HTMLElement).style.height = '440px';
    return value;
  });
  await expect.poll(distance).toBeGreaterThanOrEqual(100);
  await page.waitForTimeout(250);
  expect(await viewport.evaluate((node) => node.scrollTop)).toBe(held);
  await viewport.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
    node.dispatchEvent(new Event('scroll'));
  });
  await expect.poll(distance).toBeLessThanOrEqual(1);
  await viewport.evaluate((node) => {
    (node.firstElementChild as HTMLElement).style.height = '500px';
  });
  await expect.poll(distance).toBeLessThanOrEqual(1);
});

test('QA jump control appears only more than 120 pixels from the latest message', async ({
  page,
}) => {
  await page.goto('http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html?qa');
  await page.locator('.ol-default-capsule').waitFor();
  await page.evaluate(() => {
    const fixture = (window as any).fixture;
    fixture.emit('qa:state', { kind: 'recording', sessionId: 'follow-test', messages: [] });
    fixture.emit('qa:state', {
      kind: 'answer',
      sessionId: 'follow-test',
      messages: Array.from({ length: 12 }, (_, index) => ({
        role: index % 2 ? 'assistant' : 'user',
        content: `Message ${index}: ${'content '.repeat(30)}`,
      })),
    });
  });
  const viewport = page.locator('[data-slot="message-scroller-viewport"]');
  await expect(viewport).toBeVisible();
  const jump = page.getByRole('button', { name: '跳至最新' });
  await expect(jump).toHaveText('跳至最新');
  await expect
    .poll(() => viewport.evaluate((node) => node.scrollHeight - node.clientHeight))
    .toBeGreaterThan(150);
  await viewport.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
    node.dispatchEvent(new Event('scroll'));
  });
  await expect(jump).toHaveAttribute('data-active', 'false');
  await viewport.evaluate((node) => {
    node.dispatchEvent(new Event('touchmove'));
    node.scrollTop = node.scrollHeight - node.clientHeight - 80;
    node.dispatchEvent(new Event('scroll'));
  });
  await expect(jump).toHaveAttribute('data-active', 'false');
  await viewport.evaluate((node) => {
    node.scrollTop = node.scrollHeight - node.clientHeight - 140;
    node.dispatchEvent(new Event('scroll'));
  });
  await expect(jump).toHaveAttribute('data-active', 'true');
  await jump.click();
  await expect
    .poll(() => viewport.evaluate((node) => node.scrollHeight - node.clientHeight - node.scrollTop))
    .toBeLessThanOrEqual(1);
});
