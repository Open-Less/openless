import { expect, test, type Locator } from '@playwright/test';
import { settleNavigation } from './support/navigation';

test('default capsule follows both themes with eleven bars, including legacy style links', async ({
  page,
}, info) => {
  for (const theme of [
    { scheme: 'light' as const, fill: 'rgba(255, 255, 255, 0.86)', ink: 'rgb(29, 78, 216)' },
    { scheme: 'dark' as const, fill: 'rgba(28, 34, 45, 0.88)', ink: 'rgb(167, 199, 255)' },
  ]) {
    await page.emulateMedia({ colorScheme: theme.scheme });
    for (const style of ['classic', 'typeless']) {
      await page.goto(`/?window=capsule&style=${style}&state=recording&level=0.2`);
      const pill = page.locator('.ol-default-capsule');
      await expect(pill).toBeVisible();
      await expect(pill).toHaveCSS('color-scheme', theme.scheme);
      await expect(pill).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
      await expect(pill.locator('.ol-default-waveform > span')).toHaveCount(11);
      await expect
        .poll(() => pill.evaluate((el) => getComputedStyle(el, '::before').backgroundColor))
        .toBe(theme.fill);
      await expect(pill.locator('.ol-default-waveform > span').first()).toHaveCSS(
        'background-color',
        theme.ink,
      );
      await expect(page.locator('.ol-typeless-capsule')).toHaveCount(0);
    }
    await page.screenshot({
      path: info.outputPath(`capsule-recording-${theme.scheme}.png`),
      animations: 'disabled',
    });
  }
});

test('processing stays cancellable, hides confirm and shows a compact status', async ({
  page,
}, info) => {
  await page.goto('/?window=capsule&style=classic&state=polishing&translation=1');
  const pill = page.locator('.ol-default-capsule');
  await expect(pill).toHaveAttribute('data-processing', 'true');
  await expect(pill.locator('.ol-default-status')).toHaveCSS('opacity', '1');
  await expect(pill.locator('.ol-default-processing-cancel')).toBeEnabled();
  await expect(pill.locator('.ol-default-confirm')).toBeDisabled();
  await expect(page.locator('.ol-classic-translation')).toHaveCSS(
    'background-color',
    'rgba(28, 34, 45, 0.88)',
  );
  await expect(pill).toHaveCSS('width', '80px');
  await expect(page.locator('.ol-classic-translation')).toHaveCSS('opacity', '0');
  await page.screenshot({
    path: info.outputPath('capsule-processing.png'),
    animations: 'disabled',
  });
});

for (const theme of [
  {
    scheme: 'light' as const,
    idle: 'rgb(133, 133, 142)',
    hover: 'rgb(24, 24, 27)',
    hoverFill: 'rgb(230, 230, 231)',
    focus: 'rgb(37, 99, 235)',
    selected: 'rgb(9, 9, 11)',
    selectedFill: 'rgb(222, 222, 223)',
  },
  {
    scheme: 'dark' as const,
    idle: 'rgb(125, 125, 134)',
    hover: 'rgb(228, 228, 231)',
    hoverFill: 'rgb(28, 28, 31)',
    focus: 'rgb(116, 183, 255)',
    selected: 'rgb(250, 250, 250)',
    selectedFill: 'rgb(44, 44, 46)',
  },
]) {
  test(`sidebar distinguishes hover from selection without navigating on hover · ${theme.scheme}`, async ({
    page,
  }, info) => {
    await page.emulateMedia({ colorScheme: theme.scheme });
    await page.goto('/?os=mac');
    await expect(page.locator('html')).toHaveCSS('color-scheme', theme.scheme);
    const nav = page.locator('.ol-sidebar-surface nav');
    const history = nav.getByRole('button', { name: '历史', exact: true });
    const overview = nav.getByRole('button', { name: '概览', exact: true });
    const expectSelected = async (item: Locator) => {
      await expect(item).toHaveAttribute('aria-current', 'page');
      await expect(item).toHaveCSS('color', theme.selected);
      await expect(item).toHaveCSS('-webkit-text-fill-color', theme.selected);
      await expect(item).toHaveCSS('background-color', theme.selectedFill);
      await expect(item).toHaveCSS('box-shadow', 'none');
      await expect(item).toHaveCSS('clip-path', 'none');
    };

    await expectSelected(overview);
    await expect(history).toHaveCSS('color', theme.idle);
    await expect(history).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(history).toHaveCSS('box-shadow', 'none');
    await expect(history).toHaveCSS('cursor', 'pointer');

    await history.hover();
    await expect(history).toHaveCSS('color', theme.hover);
    await expect(history).toHaveCSS('background-color', theme.hoverFill);
    await expect(history).toHaveCSS('box-shadow', 'none');
    await expect(history).toHaveCSS('transition-duration', '0s');
    await expect(history).not.toHaveAttribute('aria-current', 'page');
    await expectSelected(overview);
    await expect(page.locator('[data-ol-page="overview"][data-transition-phase="settled"]')).toBeVisible();

    await history.click();
    await expectSelected(history);
    await expect(overview).not.toHaveAttribute('aria-current', 'page');
    await expect(page.locator('[data-ol-page="history"][data-transition-phase="settled"]')).toBeVisible();
    const styleGroup = nav.getByRole('button', { name: '风格', exact: true });
    const group = styleGroup.locator('..').locator('.ol-sidebar-nav-group');
    const chevron = styleGroup.locator('.ol-sidebar-nav-chevron');
    const polish = group.locator('.ol-nav-btn.ol-nav-subitem').filter({ hasText: '润色模式' });
    const spring = 'cubic-bezier(0.23, 1, 0.32, 1)';
    await expect(styleGroup).toHaveAttribute('aria-expanded', 'false');
    await expect(polish).toHaveAttribute('tabindex', '-1');
    await expect(group).toHaveCSS('transition-property', 'grid-template-rows');
    await expect(group).toHaveCSS('transition-duration', '0.24s');
    await expect(group).toHaveCSS('transition-timing-function', spring);
    await expect(chevron).toHaveCSS('transition-property', 'transform');
    await expect(chevron).toHaveCSS('transition-duration', '0.2s');
    await expect(chevron).toHaveCSS('transition-timing-function', spring);

    await styleGroup.click();
    await expect(styleGroup).toHaveAttribute('aria-expanded', 'true');
    await polish.hover();
    await expect(polish).toHaveCSS('background-color', theme.hoverFill);
    await expect(polish).toHaveCSS('box-shadow', 'none');
    await polish.click();
    await expectSelected(polish);
    await expect(polish).toHaveCSS('outline-style', 'none');
    await expect(history).not.toHaveAttribute('aria-current', 'page');
    await expect(page.locator('[data-ol-page="style"][data-transition-phase="settled"]')).toBeVisible();
    await page.screenshot({ path: info.outputPath('navigation.png'), animations: 'disabled' });

    await styleGroup.click();
    await expect(styleGroup).toHaveAttribute('aria-expanded', 'false');
    await expect(styleGroup).toHaveCSS('background-color', theme.selectedFill);
    await expect(group).toHaveAttribute('aria-hidden', 'true');
    await styleGroup.press('Enter');
    await expect(group).toHaveAttribute('aria-hidden', 'false');
    await page.keyboard.press('Tab');
    await expect(polish).toBeFocused();
    await expect(polish).toHaveCSS('outline-style', 'solid');
    await expect(polish).toHaveCSS('outline-color', theme.focus);
    await expectSelected(polish);
  });

  test(`Beta update controls stay flat while preserving the switch states · ${theme.scheme}`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: theme.scheme });
    await page.goto('/?os=mac');
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const settings = page.getByRole('dialog', { name: '设置', exact: true });
    await settings.getByRole('button', { name: '关于与更新', exact: true }).click();
    await settleNavigation(settings);
    const beta = settings.locator('.ol-beta-channel');
    const toggle = beta.getByRole('switch', { name: '启用 Beta 渠道' });
    const thumb = toggle.locator('.ol-switch-thumb');
    const check = beta.getByRole('button', { name: '检查 Beta 更新', exact: true });
    await expect(check).toBeEnabled();
    await expect(beta).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(beta).toHaveCSS('box-shadow', 'none');
    await expect(check).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(check).toHaveCSS('border-style', 'none');
    await expect(check).toHaveCSS('box-shadow', 'none');
    await expect(toggle).toHaveAttribute('aria-checked', 'false');
    await expect(toggle).toHaveCSS('box-shadow', 'none');
    await expect(thumb).toHaveCSS('box-shadow', 'none');
    await expect(thumb).toHaveCSS('transition-duration', '0.18s');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-checked', 'true');
    await expect(thumb).toHaveCSS('transform', 'matrix(1, 0, 0, 1, 16, 0)');
    await expect(thumb).toHaveCSS('box-shadow', 'none');
  });
}

test('rapid page choices settle immediately and reduced motion leaves no animation', async ({
  page,
}) => {
  await page.goto('/');
  await page.emulateMedia({ reducedMotion: 'reduce' });
  for (const name of ['历史', '词典', '概览']) {
    await page
      .locator('.ol-sidebar-surface nav')
      .getByRole('button', { name, exact: true })
      .click();
  }
  const content = page.locator('[data-ol-page="overview"]');
  await expect(content).toBeVisible();
  await expect(content).toHaveCSS('opacity', '1');
  expect(
    await content.evaluate((el) => ({
      inert: (el as HTMLElement).inert,
      animations: el.getAnimations().length,
    })),
  ).toEqual({ inert: false, animations: 0 });
});

test('quick follow-up uses white surfaces and blue accents even in dark mode', async ({
  page,
}, info) => {
  await page.goto('/?window=qa');
  const shell = page.locator('.qa-capsule-shell');
  await expect(shell).toBeVisible();
  expect(
    await shell.evaluate((el) => ({
      paper: getComputedStyle(el).getPropertyValue('--qa-paper').trim(),
      accent: getComputedStyle(el).getPropertyValue('--qa-accent').trim(),
    })),
  ).toEqual({ paper: '#fff', accent: '#2563eb' });
  await page.screenshot({ path: info.outputPath('quick-followup.png'), animations: 'disabled' });
});
