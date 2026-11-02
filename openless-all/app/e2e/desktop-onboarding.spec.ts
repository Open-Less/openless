import { expect, test, type Page } from '@playwright/test';

const active = (page: Page) =>
  page.locator('.ol-guide-reading:not([aria-hidden="true"]) .ol-guide-turn');
async function start(page: Page) {
  await page.goto('/?onboarding=1&os=mac');
  await page.getByRole('button', { name: '开始吧', exact: true }).click();
}
async function reachMode(page: Page) {
  await start(page);
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'mode');
}

for (const os of ['mac', 'win']) {
  test(`${os}: the complete standard guide retains answers, saves a shortcut, and hands off to the app`, async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`/?onboarding=1&os=${os}`);
    await expect(page.locator('.ol-guide-interaction')).toContainText(
      `共 ${os === 'mac' ? 8 : 7} 步`,
    );
    await page.getByRole('textbox', { name: '怎么称呼你？（选填）' }).fill('Ada');
    await page.getByRole('button', { name: '开始吧', exact: true }).click();
    await expect(page.locator('.ol-guide-history-answer')).toHaveText('Ada');
    await page.getByRole('button', { name: '体验授权' }).click();
    await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
    await page.getByRole('button', { name: '继续', exact: true }).click();
    if (os === 'mac') {
      await page.getByRole('button', { name: '稍后配置', exact: true }).click();
    } else {
      await expect(active(page)).toHaveAttribute('data-guide-step', 'mode');
      await expect(page.locator('[data-guide-step="accessibility"]')).toHaveCount(0);
    }
    await page.getByRole('button', { name: '继续', exact: true }).click();
    await page.getByRole('button', { name: '配置服务', exact: true }).click();
    await expect(page.locator('.ol-model-setup-dialog')).toBeVisible();
    await page
      .locator('.ol-model-setup-footer')
      .getByRole('button', { name: '完成', exact: true })
      .click();
    await expect(page.locator('.ol-model-setup-dialog')).toHaveCount(0);
    await page.getByRole('button', { name: '保存并继续', exact: true }).click();
    await expect(active(page)).toHaveAttribute('data-guide-step', 'polish');
    await page.getByRole('button', { name: '稍后配置', exact: true }).click();
    await expect(active(page)).toHaveAttribute('data-guide-step', 'shortcut');
    if (os === 'win') await expect(active(page)).toContainText('鼠标侧键（Mouse4 / Mouse5）');
    const recorder = active(page).locator('.ol-shortcut-recorder');
    await recorder.getByRole('button').first().click();
    if (os === 'win') {
      await page.locator('.ol-shortcut-recording').dispatchEvent('mousedown', { button: 3 });
      await expect(recorder).toContainText('Mouse4');
      await recorder.getByRole('button').first().click();
    }
    await page.keyboard.press('F8');
    await expect(recorder).toContainText('F8');
    await page.getByRole('button', { name: '就用这个快捷键', exact: true }).click();
    await expect(active(page).getByRole('heading')).toHaveAccessibleName('准备好慢慢探索了，Ada');
    await expect(active(page)).toContainText('还有一些内容留待配置');
    await page.getByRole('button', { name: '进入 OpenLess', exact: true }).click();
    await expect(page.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
    await expect(page.locator('.ol-onboarding-layer')).toHaveCount(0);
    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(errors).toEqual([]);
  });
}

test('any step can defer the whole guide without a provider nag', async ({ page }) => {
  await start(page);
  await page.getByRole('button', { name: '先去看看', exact: true }).click();
  await expect(page.getByRole('heading', { name: '今日概览', exact: true })).toBeVisible();
  await expect(page.locator('.ol-guide')).toHaveCount(0);
  await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('ol.desktopOnboarding.v1'))).toBeNull();
  await page.reload();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'welcome');
});

test('the audio-model path removes the unused writing step and never reports an unconfigured service as done', async ({
  page,
}) => {
  await reachMode(page);
  await page.getByRole('button', { name: /一个音频模型/ }).click();
  await page.getByRole('button', { name: '继续', exact: true }).click();
  await page.getByRole('button', { name: '保存并继续', exact: true }).click();
  await expect(active(page).getByRole('alert')).toContainText('请先完成服务配置');
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'shortcut');
  await expect(page.locator('[data-guide-step="polish"]')).toHaveCount(0);
});

test('a narrow reduced-motion guide preserves controls, answers, and keyboard focus', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 700 });
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await start(page);
  await expect(active(page).getByRole('heading')).toBeFocused();
  await expect(active(page)).toHaveCSS('animation-name', 'none');
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  const footer = page.locator('.ol-guide-footer');
  const bounds = await footer.boundingBox();
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(701);
  await page.getByRole('button', { name: '上一步', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'welcome');
  await expect(active(page).getByRole('textbox')).toBeVisible();
});

const fixtureUrl = 'http://127.0.0.1:5326/e2e/fixtures/desktop-onboarding.html?os=mac';
test('Windows opens microphone settings, refreshes on return, and permits deferral without a device', async ({
  page,
}) => {
  await page.goto(fixtureUrl.replace('os=mac', 'os=win'));
  await page.getByRole('button', { name: '开始吧', exact: true }).click();
  await expect(active(page)).toContainText('桌面应用访问权限');
  await page.getByRole('button', { name: '打开系统设置', exact: true }).click();
  await expect
    .poll(() => page.evaluate(() => (window as any).guideFixture.openedPanes))
    .toEqual(['microphone']);
  await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
  await page.evaluate(() => {
    (window as any).guideFixture.microphone = 'noDevice';
    window.dispatchEvent(new Event('focus'));
  });
  await expect(active(page)).toContainText('还没有检测到麦克风');
  await expect(page.locator('.ol-guide-primary')).toBeDisabled();
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'mode');
  await page.getByRole('button', { name: '上一步', exact: true }).click();
  await page.evaluate(() => {
    (window as any).guideFixture.microphone = 'granted';
    window.dispatchEvent(new Event('focus'));
  });
  await expect(active(page)).toContainText('已允许');
  await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
  await page.getByRole('button', { name: '继续', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'mode');
  const calls = await page.evaluate(() => (window as any).guideFixture.calls as string[]);
  expect(calls).not.toContain('check_accessibility_permission');
  expect(calls).not.toContain('request_accessibility_permission');
  expect(calls).not.toContain('request_microphone_permission');
});

test('native permission changes stay on the current step and progress resumes after reload', async ({
  page,
}) => {
  await page.goto(fixtureUrl);
  await page.getByRole('button', { name: '开始吧', exact: true }).click();
  await expect(page.getByRole('button', { name: '打开系统设置' })).toBeVisible();
  await page.evaluate(() => {
    (window as any).guideFixture.microphone = 'granted';
    window.dispatchEvent(new Event('focus'));
  });
  await expect(active(page)).toContainText('已允许');
  await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
  await page.reload();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await page.getByRole('button', { name: '先去看看', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Guide completed' })).toBeVisible();
  expect(
    await page.evaluate(
      () => JSON.parse(localStorage.getItem('ol.desktopOnboarding.v1')!).finished,
    ),
  ).toBe(false);
});

test('native saves and permission failures remain retryable without advancing', async ({
  page,
}) => {
  await page.goto(fixtureUrl);
  await page.getByRole('button', { name: '开始吧', exact: true }).click();
  await page.evaluate(() => {
    (window as any).guideFixture.failPermission = true;
  });
  await page.getByRole('button', { name: '重新检查', exact: true }).click();
  await expect(active(page).getByRole('alert')).toBeVisible();
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await page.evaluate(() => {
    (window as any).guideFixture.failSave = true;
  });
  await page.getByRole('button', { name: /一个音频模型/ }).click();
  await expect(active(page).getByRole('alert')).toBeVisible();
  await expect(page.getByRole('button', { name: /语音识别 \+ 文字润色/ })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.evaluate(() => {
    (window as any).guideFixture.failSave = false;
  });
  await page.getByRole('button', { name: '继续', exact: true }).click();
  await page.getByRole('button', { name: '保存并继续', exact: true }).click();
  await expect(active(page).getByRole('alert')).toContainText('请先完成服务配置');
  await page.evaluate(() => {
    (window as any).guideFixture.serviceReady = true;
  });
  await page.getByRole('button', { name: '保存并继续', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'polish');
});

test('the main action stays in place while the current step moves to the center', async ({
  page,
}) => {
  await page.goto('/?onboarding=1&os=mac');
  const button = page.locator('.ol-guide-interaction .ol-guide-primary');
  const before = (await button.boundingBox())!;
  await button.click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'microphone');
  await expect
    .poll(async () => Math.abs((await button.boundingBox())!.y - before.y))
    .toBeLessThan(1);
  await page.waitForTimeout(550);
  const region = (await active(page).boundingBox())!;
  const viewport = page.viewportSize()!;
  expect(Math.abs(region.y + region.height / 2 - viewport.height / 2)).toBeLessThan(65);
  await page.getByRole('button', { name: '稍后配置', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'accessibility');
  expect(Math.abs((await button.boundingBox())!.y - before.y)).toBeLessThan(1);
});

test('text reveals progressively without shifting layout and input focus uses an underline', async ({
  page,
}) => {
  await page.goto('/?onboarding=1&os=mac');
  const input = page.getByRole('textbox', { name: '怎么称呼你？（选填）' });
  await input.click();
  await expect(input).toHaveCSS('outline-style', 'none');
  const before = (await input.boundingBox())!;
  const reveal = await page.locator('.ol-reveal-glyph').evaluateAll((elements) =>
    elements
      .map((element) => {
        const animation = element.getAnimations()[0];
        return animation?.effect?.getTiming();
      })
      .filter(Boolean),
  );
  expect(reveal.length).toBeGreaterThan(20);
  expect(Math.max(...reveal.map((timing) => Number(timing!.delay)))).toBeGreaterThan(1800);
  await expect
    .poll(() =>
      page
        .locator('.ol-reveal-glyph')
        .last()
        .evaluate((element) => getComputedStyle(element).opacity),
    )
    .toBe('1');
  const after = (await input.boundingBox())!;
  expect(Math.abs(before.y - after.y)).toBeLessThan(1);
});

test('model setup stays in its own dialog and returns to the unchanged centered step', async ({
  page,
}) => {
  await reachMode(page);
  await page.getByRole('button', { name: '继续', exact: true }).click();
  await expect(active(page)).toHaveAttribute('data-guide-step', 'recognition');
  await expect(page.locator('.ol-guide-reading[data-transition-phase="settled"]')).toHaveCount(1);
  const before = (await active(page).boundingBox())!;
  const actionsBefore = (await page.locator('.ol-guide-interaction').boundingBox())!;
  expect(before.y + before.height).toBeLessThanOrEqual(actionsBefore.y + 1);
  await page.getByRole('button', { name: '配置服务', exact: true }).click();
  const popup = page.locator('.ol-model-setup-dialog');
  await expect(popup).toBeVisible();
  await expect(active(page).locator('.ol-reorder-list')).toHaveCount(0);
  await popup.getByRole('button', { name: '添加渠道', exact: true }).click();
  const editor = popup.getByRole('dialog', { name: '添加渠道', exact: true });
  expect(await editor.evaluate((element) => !!element.closest('[inert]'))).toBe(false);
  await editor.getByRole('combobox', { name: '供应商', exact: true }).click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.keyboard.press('Escape');
  await editor.getByRole('button', { name: '返回渠道列表', exact: true }).click();
  await expect(editor).toHaveCount(0);
  await popup
    .locator('.ol-model-setup-footer')
    .getByRole('button', { name: '完成', exact: true })
    .click();
  await expect(popup).toHaveCount(0);
  expect(Math.abs((await active(page).boundingBox())!.y - before.y)).toBeLessThan(1);
  expect(
    Math.abs((await page.locator('.ol-guide-interaction').boundingBox())!.y - actionsBefore.y),
  ).toBeLessThan(1);
});
