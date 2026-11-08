import { expect, test } from '@playwright/test';
import { openCapsuleStyleSettings, settleNavigation } from './support/navigation';

test('recording capsule retains its size and original pill geometry with translation', async ({
  page,
}) => {
  await page.goto('http://127.0.0.1:5326/e2e/fixtures/capsule-motion.html?full&os=mac');
  await page.waitForFunction(() => (window as any).fixture.hasListener('capsule:state'));
  await page.evaluate(() =>
    (window as any).fixture.emit('capsule:state', {
      state: 'recording',
      level: 0.25,
      translation: true,
      sessionId: 'geometry',
    }),
  );
  const capsule = page.locator('.ol-default-capsule');
  await expect(capsule).toBeVisible();
  await expect(capsule).toHaveCSS('width', '116px');
  await expect(capsule).toHaveCSS('height', '32px');
  await expect(capsule).toHaveCSS('border-radius', '999px');
  await expect(capsule).toHaveCSS('clip-path', 'none');
  for (const button of await capsule.locator('.ol-default-action').all()) {
    await expect(button).toHaveCSS('border-radius', '50%');
    await expect(button).toHaveCSS('clip-path', 'none');
  }
  await page.getByRole('combobox', { name: '翻译目标语言' }).click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await expect(capsule).toHaveCSS('width', '116px');
  await expect(capsule).toHaveCSS('height', '32px');
});

test('settings switches, segmented indicators and popups keep their full outlines', async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置' });
  await dialog.getByRole('button', { name: 'AI 与模型', exact: true }).click();
  await settleNavigation(page);
  const group = dialog.getByRole('group', { name: '识别管线' });
  const indicator = group.locator('.ol-segmented-indicator');
  await expect(indicator).toHaveCSS('clip-path', 'none');
  await expect(group).toHaveCSS('clip-path', 'none');
  await group.getByRole('button', { name: '多模态模式' }).click();
  const frames = await group.evaluate(async (track) => {
    const indicator = track.querySelector<HTMLElement>('.ol-segmented-indicator')!;
    const result: { outside: number; scale: number }[] = [];
    for (let i = 0; i < 18; i++) {
      await new Promise(requestAnimationFrame);
      const outer = track.getBoundingClientRect();
      const inner = indicator.getBoundingClientRect();
      result.push({
        outside: Math.max(0, outer.left - inner.left, inner.right - outer.right),
        scale: new DOMMatrixReadOnly(getComputedStyle(indicator).transform).a,
      });
    }
    return result;
  });
  expect(Math.max(...frames.map((frame) => frame.outside))).toBeLessThan(2);
  expect(frames.every((frame) => Math.abs(frame.scale - 1) < 0.01)).toBe(true);
  await expect
    .poll(() =>
      indicator.evaluate((node) => {
        const selected = node.parentElement!.querySelector<HTMLElement>(
          'button[aria-pressed="true"]',
        )!;
        const a = node.getBoundingClientRect(),
          b = selected.getBoundingClientRect();
        return Math.max(Math.abs(a.left - b.left), Math.abs(a.width - b.width));
      }),
    )
    .toBeLessThan(1.5);
  expect(
    await indicator.evaluate((node) => new DOMMatrixReadOnly(getComputedStyle(node).transform).a),
  ).toBeCloseTo(1, 2);
  await dialog.getByRole('button', { name: '外观与语言', exact: true }).click();
  await settleNavigation(page);
  const toggle = dialog.getByRole('switch', { name: '概览页显示年度活动热力图' });
  await expect(toggle).toHaveCSS('border-radius', '999px');
  await expect(toggle.locator('.ol-switch-thumb')).toHaveCSS('border-radius', '999px');
  await expect(toggle).toHaveCSS('clip-path', 'none');
  const select = dialog.getByRole('combobox', { name: '字体系列' });
  await expect(select).toHaveCSS('clip-path', 'none');
  expect(await select.evaluate((node) => getComputedStyle(node).backgroundColor)).not.toBe(
    'rgba(0, 0, 0, 0)',
  );
  const before = await select.innerText();
  await select.click();
  const popup = page.locator('.ol-select-popup');
  await expect(popup).toBeVisible();
  await expect(popup).toHaveCSS('clip-path', 'none');
  await dialog.getByText('字体与排版', { exact: true }).click();
  await expect(popup).toBeHidden();
  await expect(select).toHaveText(before);
  await select.click();
  await page.getByRole('option', { name: 'Avenir Next · Aptos' }).click();
  await expect(select).toBeFocused();
  await expect(select).toHaveCSS('outline-style', 'none');
  await page.keyboard.press('Tab');
  await page.keyboard.press('Shift+Tab');
  await expect(select).toBeFocused();
  await expect(select).toHaveCSS('outline-style', 'solid');
  const theme = dialog.getByRole('group', { name: '主题' });
  await expect(theme).toHaveCSS('clip-path', 'none');
  await expect(theme.locator('.ol-segmented-indicator')).toHaveCSS('clip-path', 'none');
  await page.screenshot({ path: test.info().outputPath('appearance-controls.png') });
  await dialog.getByRole('button', { name: '通用', exact: true }).click();
  await openCapsuleStyleSettings(dialog);
  const capsuleGroup = dialog.locator('.ol-capsule-style-segmented');
  await expect(capsuleGroup).toBeVisible();
  await expect(capsuleGroup).toHaveCSS('clip-path', 'none');
  await expect(capsuleGroup.locator('.ol-segmented-indicator')).toHaveCSS('clip-path', 'none');
  await expect(dialog.locator('.ol-default-capsule')).toHaveCSS('border-radius', '999px');
  await page.screenshot({ path: test.info().outputPath('capsule-settings.png') });
});

test('history list keeps its rounded surface and native trackpad scrolling; correction add uses 8px', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await settleNavigation(page);
  const list = page.locator('.ol-history-list');
  await expect(list).toBeVisible();
  const colors = await list.evaluate((node) => ({
    surface: getComputedStyle(node).backgroundColor,
    header: getComputedStyle(node.querySelector('.ol-thinscroll > div')!).backgroundColor,
  }));
  expect(colors.header).toBe(colors.surface);
  const wheelPrevented = await list.locator('.ol-thinscroll').evaluate((node) => {
    const event = new WheelEvent('wheel', { deltaY: 13.25, bubbles: true, cancelable: true });
    node.dispatchEvent(event);
    return event.defaultPrevented;
  });
  expect(wheelPrevented).toBe(false);
  await page.screenshot({ path: test.info().outputPath('history-list.png') });
  await page.getByRole('button', { name: '工具', exact: true }).click();
  await page.getByRole('button', { name: '纠正规则', exact: true }).click();
  await settleNavigation(page);
  const add = page.getByRole('button', { name: '添加', exact: true });
  await expect(add).toBeVisible();
  await expect(add).toHaveCSS('border-radius', '8px');
  await expect(add).toHaveCSS('clip-path', 'none');
  await page.screenshot({ path: test.info().outputPath('correction-add.png') });
  expect(
    await add.evaluate((node) =>
      getComputedStyle(node).getPropertyValue('--ol-corner-radius').trim(),
    ),
  ).toBe('8px');
});

test('the WebKit fallback preserves capsule, switch and popup exceptions', async ({ page }) => {
  await page.goto('http://127.0.0.1:5326/e2e/fixtures/refinements.html');
  const result = await page.evaluate(async () => {
    const css = await (await fetch('/src/styles/continuous-corners.css?direct')).text();
    const source = document.createElement('style');
    source.textContent = css;
    document.head.append(source);
    const rules = Array.from(source.sheet!.cssRules);
    const fallback = rules.find(
      (rule) => rule instanceof CSSSupportsRule && rule.conditionText.includes('not (corner-shape'),
    ) as CSSSupportsRule | undefined;
    if (!fallback) throw new Error('continuous-corner fallback missing');
    const exceptions = rules.filter(
      (rule): rule is CSSStyleRule =>
        rule instanceof CSSStyleRule &&
        rule.style.getPropertyValue('clip-path') === 'none' &&
        (rule.selectorText.includes('.ol-default-capsule') ||
          rule.selectorText.includes('.ol-default-action') ||
          rule.selectorText.includes('.ol-select-popup') ||
          rule.selectorText.includes('.ol-correction-add')),
    );
    if (exceptions.length !== 4)
      throw new Error(`expected four fallback exceptions, got ${exceptions.length}`);
    const force = document.createElement('style');
    force.textContent =
      Array.from(fallback.cssRules)
        .map((rule) => rule.cssText)
        .join('\n') +
      '\n' +
      exceptions.map((rule) => rule.cssText).join('\n');
    document.head.append(force);
    const holder = document.createElement('div');
    holder.innerHTML =
      '<div class="ol-default-capsule"></div><button class="ol-default-action"></button><button class="ol-switch"></button><div class="ol-switch-thumb"></div><div class="ol-select-popup"></div><button class="ol-correction-add"></button>';
    document.body.append(holder);
    const read = (selector: string) => {
      const style = getComputedStyle(holder.querySelector(selector)!);
      return { radius: style.borderRadius, clip: style.clipPath };
    };
    const values = {
      capsule: read('.ol-default-capsule'),
      action: read('.ol-default-action'),
      toggle: read('.ol-switch'),
      thumb: read('.ol-switch-thumb'),
      popup: read('.ol-select-popup'),
      add: read('.ol-correction-add'),
    };
    holder.remove();
    force.remove();
    source.remove();
    return values;
  });
  expect(result).toEqual({
    capsule: { radius: '999px', clip: 'none' },
    action: { radius: '50%', clip: 'none' },
    toggle: { radius: '999px', clip: 'none' },
    thumb: { radius: '999px', clip: 'none' },
    popup: { radius: '16px', clip: 'none' },
    add: { radius: '8px', clip: 'none' },
  });
});
