import { settleNavigation } from './support/navigation';
import { expect, test, type ElementHandle, type Locator, type Page } from '@playwright/test';

// Every switch the user can reach in Settings (plus the services channel lists and the
// Selection Ask page) must slide its knob between on and off, never jump. Each switch is
// flipped twice so both directions are sampled and the mock preferences end unchanged.

const errors = new WeakMap<Page, string[]>();
test.beforeEach(async ({ page }) => {
  const pageErrors: string[] = [];
  errors.set(page, pageErrors);
  page.on('pageerror', (error) => pageErrors.push(error.message));
  await page.emulateMedia({ reducedMotion: 'no-preference' });
  await page.goto('/?os=mac');
  await expect(page.getByRole('button', { name: '设置', exact: true })).toBeVisible();
});
test.afterEach(async ({ page }) => {
  expect(errors.get(page)).toEqual([]);
});

type Sample = {
  name: string;
  checked: [string, string];
  start: number;
  end: number;
  xs: number[];
  /** Knob positions at controlled points of the real CSS transition. */
  controlledXs: number[];
  /** elapsedTime (s) of every completed knob transform transition. */
  finished: number[];
};

/** Clicks a switch and records its knob's left edge on every animation frame. Works on the
 * element itself: a channel row may move within its list once its switch flips. */
async function flip(page: Page, control: ElementHandle<HTMLElement>): Promise<Sample> {
  await control.scrollIntoViewIfNeeded();
  // Pointer input: keyboard mode deliberately disables the knob transition.
  const box = (await control.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  const before = (await control.getAttribute('aria-checked')) ?? '';
  const name = await control.evaluate((element) => {
    const labelledBy = element.getAttribute('aria-labelledby');
    return (
      element.getAttribute('aria-label') ??
      (labelledBy ? document.getElementById(labelledBy)?.textContent : null) ??
      ''
    ).trim();
  });
  const start = await control.evaluate((element) => {
    const knob = element.querySelector<HTMLElement>('.ol-switch-thumb')!;
    return knob.getBoundingClientRect().left;
  });
  // Arm the sampling listeners and *wait* for them before clicking. `evaluate` only
  // dispatches its function; the click is a separate protocol message and on a loaded CI
  // runner it can reach the page first. When that happened the `transitionrun` listener was
  // not attached yet, so `controlledXs` came back empty — "语音编辑 exposed its transform
  // transition", which failed 6 of 43 CI runs across unrelated branches, beta included. The
  // switch animated every one of those times; only the hook missed it.
  await control.evaluate((element) => {
    const knob = element.querySelector<HTMLElement>('.ol-switch-thumb')!;
    const state = { finished: [] as number[], controlledXs: [] as number[] };
    const onRun = (event: TransitionEvent) => {
      if (event.target !== knob || event.propertyName !== 'transform') return;
      const transition = knob
        .getAnimations()
        .find(
          (animation) =>
            animation instanceof CSSTransition && animation.transitionProperty === 'transform',
        );
      if (!transition?.effect) return;
      const duration = transition.effect.getComputedTiming().duration;
      if (typeof duration !== 'number' || duration <= 0) return;
      // CI can paint only every 80–100ms, missing the useful part of a 180ms ease-out.
      // Seek within the real transition without pausing it. Pausing and playing can defer
      // WebKit's restart until another paint, extending the slide into the row reorder.
      // Restore the original timeline so sampling cannot delay normal completion.
      const startTime = transition.startTime;
      const currentTime = transition.currentTime;
      for (const fraction of [0.25, 0.5, 0.75]) {
        transition.currentTime = duration * fraction;
        state.controlledXs.push(knob.getBoundingClientRect().left);
      }
      if (startTime !== null) transition.startTime = startTime;
      else transition.currentTime = currentTime;
    };
    const onEnd = (event: TransitionEvent) => {
      if (event.target === knob && event.propertyName === 'transform') {
        state.finished.push(event.elapsedTime);
      }
    };
    knob.addEventListener('transitionrun', onRun);
    knob.addEventListener('transitionend', onEnd);
    const store = window as unknown as {
      __switchMotion?: WeakMap<Element, { state: typeof state; stop: () => void }>;
    };
    store.__switchMotion ??= new WeakMap();
    store.__switchMotion.set(element, {
      state,
      stop: () => {
        knob.removeEventListener('transitionrun', onRun);
        knob.removeEventListener('transitionend', onEnd);
      },
    });
  });
  await page.mouse.down();
  await page.mouse.up();
  // The frame loop starts *after* the click: 45 frames (~750ms) still covers the 180ms slide
  // and settles, so `xs.at(-1)` is the resting position regardless of how long the click takes
  // to arrive. Listening already started above, so a late click cannot lose the transition.
  const { xs, controlledXs, finished } = await control.evaluate(async (element) => {
    const knob = element.querySelector<HTMLElement>('.ol-switch-thumb')!;
    const armed = (
      window as unknown as {
        __switchMotion: WeakMap<
          Element,
          { state: { finished: number[]; controlledXs: number[] }; stop: () => void }
        >;
      }
    ).__switchMotion.get(element)!;
    const xs: number[] = [];
    for (let frame = 0; frame < 45; frame++) {
      await new Promise<number>(requestAnimationFrame);
      // A remount would leave this node detached and frozen: count it as a jump.
      xs.push(element.isConnected ? knob.getBoundingClientRect().left : Number.NaN);
    }
    armed.stop();
    return { xs, controlledXs: armed.state.controlledXs, finished: armed.state.finished };
  });
  const after = (await control.getAttribute('aria-checked')) ?? '';
  return { name, checked: [before, after], start, end: xs.at(-1)!, xs, controlledXs, finished };
}

function expectAnimated(sample: Sample) {
  const { name, checked, start, end, controlledXs, finished } = sample;
  expect(checked[1], `${name} changed state`).not.toBe(checked[0]);
  expect(Math.abs(end - start), `${name} knob travelled`).toBeGreaterThan(4);
  // The knob's transform transition ran to completion (a jump or remount has none).
  expect(
    finished.some((elapsed) => elapsed >= 0.1),
    `${name} knob finished a transform transition`,
  ).toBe(true);
  const low = Math.min(start, end) + 0.5;
  const high = Math.max(start, end) - 0.5;
  expect(controlledXs, `${name} exposed its transform transition`).toHaveLength(3);
  expect(
    controlledXs.some((x) => x > low && x < high),
    `${name} knob passed through controlled intermediate positions (${start} → ${controlledXs.join(' ')} → ${end})`,
  ).toBe(true);
}

async function exerciseVisibleSwitches(scope: Locator, label: string) {
  await settleNavigation(scope.page());
  const page = scope.page();
  // Handles, not positions: flipping a channel can move its row within the list.
  const switches = await scope.locator('[role="switch"]').elementHandles();
  let exercised = 0;
  for (const control of switches as ElementHandle<HTMLElement>[]) {
    if (!(await control.isVisible())) continue;
    const unavailable = await control.evaluate(
      (element) =>
        element.hasAttribute('disabled') ||
        element.hasAttribute('data-disabled') ||
        element.getAttribute('aria-disabled') === 'true' ||
        // Collapsed groups keep their rows laid out but inert.
        element.closest('[inert]') !== null,
    );
    if (unavailable) continue;
    expectAnimated(await flip(page, control));
    expectAnimated(await flip(page, control));
    exercised++;
  }
  test.info().annotations.push({ type: 'switches', description: `${label}: ${exercised}` });
  return exercised;
}

async function openSettings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(page.locator('.ol-settings-workspace')).toHaveCSS('opacity', '1');
  await expect(page.locator('.ol-settings-workspace')).toHaveCSS('transform', 'none');
  await settleNavigation(dialog);
  return dialog;
}

async function openSection(dialog: Locator, name: string) {
  await dialog
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button', { name, exact: true })
    .click();
  await settleNavigation(dialog);
}

test('every settings switch slides between on and off', async ({ page }) => {
  test.setTimeout(180_000);
  const dialog = await openSettings(page);
  const sections = await dialog
    .getByRole('navigation', { name: '设置分类' })
    .getByRole('button')
    .allTextContents();
  expect(sections.length).toBeGreaterThan(4);
  let total = 0;
  for (const section of sections.map((name) => name.trim())) {
    await openSection(dialog, section);
    // Open every collapsible group so its switches are reachable.
    const headers = dialog.locator('[data-ol-toggle-group] > button[aria-expanded]');
    for (let index = 0; index < (await headers.count()); index++) {
      const header = headers.nth(index);
      if ((await header.getAttribute('aria-expanded')) !== 'false') continue;
      await header.click();
      await expect(header).toHaveAttribute('aria-expanded', 'true');
    }
    if (section === '实验与扩展') {
      const entries = dialog.locator('[data-ol-advanced-entry]');
      const visibleEntries = () =>
        entries.evaluateAll((items) =>
          items
            .filter((item) => item.getClientRects().length > 0)
            .map((item) => item.getAttribute('data-ol-advanced-entry')!),
        );
      const visit = async (id: string, inside?: () => Promise<void>) => {
        await dialog.locator(`[data-ol-advanced-entry="${id}"]`).click();
        await expect(dialog.locator('.ol-settings-back')).toBeVisible();
        // Pages visited earlier stay mounted but hidden; only the open page is visible.
        total += await exerciseVisibleSwitches(dialog, `advanced:${id}`);
        await inside?.();
        await dialog.locator('.ol-settings-back').click();
        await expect(entries.first()).toBeVisible();
        await settleNavigation(dialog);
      };
      for (const id of await visibleEntries()) {
        await visit(id, async () => {
          if (id !== 'testingFeatures') return;
          // Testing pages are only listed while their switch is on.
          const enable = dialog.getByRole('switch', { name: '启用正在测试中的功能' });
          await enable.click();
          await expect(enable).toHaveAttribute('aria-checked', 'true');
          await expect(entries.first()).toBeVisible();
          for (const testingId of await visibleEntries()) await visit(testingId);
          await enable.click();
          await expect(enable).toHaveAttribute('aria-checked', 'false');
        });
      }
      continue;
    }
    total += await exerciseVisibleSwitches(dialog, section);
  }
  expect(total).toBeGreaterThan(15);
});

test('channel enable switches slide in every services list', async ({ page }) => {
  const dialog = await openSettings(page);
  await openSection(dialog, 'AI 服务与模型');
  let total = 0;
  for (const view of ['语言模型', '语音识别']) {
    await dialog
      .locator('.ol-service-views')
      .getByRole('button', { name: new RegExp(`^${view}`) })
      .click();
    const list = dialog.locator('.ol-reorder-list');
    await expect(list.locator('[role="switch"]').first()).toBeVisible();
    total += await exerciseVisibleSwitches(list, view);
  }
  expect(total).toBeGreaterThanOrEqual(2);
});

test('channel sorting waits for a slow knob transition to finish', async ({ page }) => {
  const dialog = await openSettings(page);
  await openSection(dialog, 'AI 服务与模型');
  await page.addStyleTag({
    content: '.ol-switch-thumb { transition-duration: 700ms !important; }',
  });
  const first = dialog.locator('.ol-reorder-list [role="switch"][aria-checked="true"]').first();
  await expect(first).toBeVisible();
  const name = (await first.getAttribute('aria-label'))!;
  const control = dialog.getByRole('switch', { name, exact: true });
  await control.evaluate((element) => {
    const knob = element.querySelector<HTMLElement>('.ol-switch-thumb')!;
    element.setAttribute('data-test-slide', 'pending');
    for (const type of ['transitionend', 'transitioncancel']) {
      knob.addEventListener(type, (event) => {
        if ((event as TransitionEvent).propertyName === 'transform') {
          element.setAttribute('data-test-slide', type);
        }
      });
    }
  });
  await control.click();
  await expect(control).toHaveAttribute('aria-checked', 'false');
  await expect(control).toHaveAttribute('data-test-slide', 'transitionend');
});

test('the Selection Ask history switch slides', async ({ page }) => {
  await page.getByRole('button', { name: '工具', exact: true }).click();
  await page.getByRole('button', { name: '问任何问题', exact: true }).click();
  const main = page.locator('main');
  await expect(main.getByRole('switch').first()).toBeVisible();
  expect(await exerciseVisibleSwitches(main, 'selectionAsk')).toBeGreaterThan(0);
});

test('stacked switches ripple their neighbours in collapsible groups and channel lists', async ({
  page,
}) => {
  const dialog = await openSettings(page);
  const neighbourMoves = async (group: Locator) => {
    const switches = group.locator('[role="switch"]');
    await expect(switches.nth(1)).toBeVisible();
    await switches.nth(0).click();
    return switches.nth(1).evaluate((element) => element.getAnimations().length);
  };
  const startup = dialog.getByRole('button', { name: '启动', exact: true });
  if ((await startup.getAttribute('aria-expanded')) === 'false') await startup.click();
  // The collapsible's own root is its group: switches ripple only within that list.
  const startupGroup = startup.locator('xpath=..');
  await expect(startupGroup).toHaveAttribute('data-ol-toggle-group', '');
  expect(await neighbourMoves(startupGroup)).toBeGreaterThan(0);
  await openSection(dialog, 'AI 服务与模型');
  const channels = dialog.locator('.ol-reorder-list[data-ol-toggle-group]');
  expect(await neighbourMoves(channels)).toBeGreaterThan(0);
});
