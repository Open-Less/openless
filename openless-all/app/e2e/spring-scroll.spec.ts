import { expect, test } from '@playwright/test';

test.use({ hasTouch: true });

test('nested scrolling carries touch velocity and rebounds at its edge', async ({ page }) => {
  await page.goto('http://127.0.0.1:5326/');
  await page.evaluate(() => {
    const outer = document.createElement('div');
    outer.id = 'spring-outer';
    outer.style.cssText =
      'position:fixed;z-index:99999;left:20px;top:20px;width:240px;height:220px;overflow:auto;background:white';
    outer.innerHTML =
      '<div style="height:100px"></div><div id="spring-inner" style="width:200px;height:100px;overflow:auto"><div style="height:1000px"></div></div><div style="height:600px"></div>';
    document.body.append(outer);
  });
  const inner = page.locator('#spring-inner');
  const before = await inner.evaluate((node) => {
    const send = (type: string, y: number, ended = false) => {
      const touch = { identifier: 1, target: node, clientX: 60, clientY: y };
      const event = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperty(event, 'touches', { value: ended ? [] : [touch] });
      Object.defineProperty(event, 'changedTouches', { value: [touch] });
      node.dispatchEvent(event);
    };
    send('touchstart', 180);
    send('touchmove', 140);
    const dragged = node.scrollTop;
    send('touchend', 140, true);
    return dragged;
  });
  expect(before).toBeGreaterThan(0);
  await expect.poll(() => inner.evaluate((node) => node.scrollTop)).toBeGreaterThan(before + 5);
  expect(await page.locator('#spring-outer').evaluate((node) => node.scrollTop)).toBe(0);
  await page.waitForTimeout(1000);
  await inner.evaluate((node) => {
    node.scrollTop = 0;
  });
  const bounced = await inner.evaluate(async (node) => {
    const wheel = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -120 });
    node.dispatchEvent(wheel);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return { prevented: wheel.defaultPrevented, translate: (node as HTMLElement).style.translate };
  });
  expect(bounced.prevented).toBe(true);
  expect(bounced.translate).not.toBe('');
  await expect.poll(() => inner.evaluate((node) => (node as HTMLElement).style.translate)).toBe('');
});

test('reduced motion leaves scrolling to the browser', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('http://127.0.0.1:5326/');
  const prevented = await page.evaluate(() => {
    const node = document.createElement('div');
    node.style.cssText = 'position:fixed;width:100px;height:100px;overflow:auto';
    node.innerHTML = '<div style="height:1000px"></div>';
    document.body.append(node);
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 100 });
    node.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(prevented).toBe(false);
});
