import { expect, test } from '@playwright/test';

test('default capsule is white and blue with eleven bars, including legacy style links', async ({
  page,
}, info) => {
  for (const style of ['classic', 'typeless']) {
    await page.goto(`/?window=capsule&style=${style}&state=recording&level=0.2`);
    const pill = page.locator('.ol-default-capsule');
    await expect(pill).toBeVisible();
    await expect(pill).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await expect(pill.locator('.ol-default-waveform > span')).toHaveCount(11);
    expect(await pill.evaluate((el) => getComputedStyle(el, '::before').backgroundColor)).toBe(
      'rgb(255, 255, 255)',
    );
    await expect(pill.locator('.ol-default-waveform > span').first()).toHaveCSS(
      'background-color',
      'rgb(37, 99, 235)',
    );
    await expect(page.locator('.ol-typeless-capsule')).toHaveCount(0);
  }
  await page.screenshot({ path: info.outputPath('capsule-recording.png'), animations: 'disabled' });
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
    'rgb(255, 255, 255)',
  );
  await expect(pill).toHaveCSS('width', '80px');
  await expect(page.locator('.ol-classic-translation')).toHaveCSS('opacity', '0');
  await page.screenshot({
    path: info.outputPath('capsule-processing.png'),
    animations: 'disabled',
  });
});

test('sidebar distinguishes hover from selection without navigating on hover', async ({
  page,
}, info) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  await expect(page.locator('html')).not.toHaveAttribute('data-ol-theme', 'dark');
  const nav = page.locator('.ol-sidebar-surface nav');
  const history = nav.getByRole('button', { name: '历史', exact: true });
  const overview = nav.getByRole('button', { name: '概览', exact: true });
  // Selected: black on the darker wash. Idle: light gray, no wash. Hover: near-black on a
  // lighter wash than the selection, with a pointer.
  await expect(overview).toHaveCSS('color', 'rgb(0, 0, 0)');
  await expect(overview).toHaveCSS('background-color', 'rgba(0, 0, 0, 0.075)');
  await expect(history).toHaveCSS('color', 'rgb(133, 133, 142)');
  await expect(history).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(history).toHaveCSS('cursor', 'pointer');
  await history.hover();
  await expect(history).toHaveCSS('color', 'rgb(24, 24, 27)');
  await expect(history).toHaveCSS('background-color', 'rgba(0, 0, 0, 0.04)');
  await expect(page.locator('[data-ol-page="overview"]')).toBeVisible();
  await history.click();
  await expect(history).toHaveAttribute('aria-current', 'page');
  await expect(history).toHaveCSS('color', 'rgb(0, 0, 0)');
  await expect(history).toHaveCSS('background-color', 'rgba(0, 0, 0, 0.075)');
  await expect(page.locator('[data-ol-page="history"]')).toHaveCSS('opacity', '1');
  await page.screenshot({ path: info.outputPath('navigation.png'), animations: 'disabled' });
});

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
