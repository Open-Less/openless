import { expect, test, type Page } from '@playwright/test';
import { expectOverviewContained } from './support/overview';
import { fixtureOrigin } from './support/server';

test('native overview paints cached activity and discards an app list invalidated by a newer revision', async ({
  page,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem(
      'ol-overview-status-v1',
      JSON.stringify({
        activity: [{ date: `${new Date().getFullYear()}-01-01`, count: 30 }],
      }),
    );
  });
  await page.goto(`${fixtureOrigin}/e2e/fixtures/native-overview.html`);
  const heading = page.locator('.ol-gha-heading');
  await expect(heading).toContainText('30 dictations');
  const chips = page.locator('.ol-gha-app-chips');
  await expect(chips).toContainText('Editor');
  expect(await chips.locator(':scope > *').first().textContent()).toContain('Editor');
  await page.getByRole('button', { name: 'Complete initial activity read' }).click();
  await expect(heading).toContainText('2 dictations');

  await page.getByRole('button', { name: 'Publish record write' }).click();
  const appReads = page.getByRole('button', { name: /App read count|^\d+$/ });
  await expect
    .poll(async () => {
      await appReads.click();
      return appReads.textContent();
    })
    .toBe('2');
  await page.getByRole('button', { name: 'Publish activity write' }).click();
  await expect(heading).toContainText('43 dictations');
  await appReads.click();
  await expect(appReads).toHaveText('2');
  await expect(chips).toContainText('Editor2');
  await page.getByRole('button', { name: 'Complete old app read' }).click();
  await expect(chips).toContainText('43');
  await appReads.click();
  await expect(appReads).toHaveText('3');
});

async function expectDashboardFits(page: Page) {
  const dashboard = page.locator('.ol-overview-dashboard');
  await expect(dashboard).toBeVisible();
  for (const owner of [
    page.locator('#root'),
    page.locator('[data-ol-page="overview"]'),
    dashboard,
  ]) {
    await expect
      .poll(() => owner.evaluate((node) => node.scrollHeight - node.clientHeight))
      .toBeLessThanOrEqual(1);
    expect(await owner.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(
      1,
    );
  }
  await expectOverviewContained(dashboard.locator('.ol-overview-period-controls'));
  await expectOverviewContained(
    dashboard.locator('.ol-overview-period-summary, .ol-overview-period button').first(),
  );
  const dates = dashboard.locator('.ol-overview-period-dates');
  if (await dates.count()) await expectOverviewContained(dates);
  const cells = dashboard.locator('.ol-gha-cell');
  if (await cells.count()) {
    await expect(dashboard.locator('.ol-gha-week')).toHaveCount(53);
    expect(await dashboard.locator('.ol-gha-grid').evaluate((node) => node.scrollLeft)).toBe(0);
    await expectOverviewContained(dashboard.locator('.ol-gha-week').first());
    await expectOverviewContained(dashboard.locator('.ol-gha-week').last());
    await expectOverviewContained(dashboard.locator('.ol-overview-year'));
    for (const value of await dashboard.locator('.ol-overview-year-stat').all()) {
      await expectOverviewContained(value);
    }
    await expect
      .poll(() =>
        dashboard.locator('.ol-gha-grid').evaluate((node) => node.scrollWidth - node.clientWidth),
      )
      .toBeLessThanOrEqual(1);
    const toggle = dashboard.locator('.ol-gha-toggle');
    if (await toggle.count()) await expectOverviewContained(toggle);
  }
}

for (const size of [
  { width: 1400, height: 900 },
  { width: 1300, height: 835 },
  { width: 980, height: 640 },
]) {
  for (const scale of ['small', 'medium', 'large']) {
    test(`the native dashboard fits ${size.width}×${size.height} at ${scale} scale`, async ({
      page,
    }, testInfo) => {
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await page.setViewportSize(size);
      await page.goto(
        `${fixtureOrigin}/e2e/fixtures/native-overview.html?dashboard&scale=${scale}`,
      );
      await expect(page.locator('[data-overview-record]')).toHaveCount(40);
      await expect(page.locator('.ol-gha-heading')).toContainText('987654321');
      await expectDashboardFits(page);
      await expectOverviewContained(page.locator('.ol-overview-period-dates'));
      await page.getByRole('button', { name: 'Last 30 days', exact: true }).click();
      await page.getByRole('button', { name: 'Duration', exact: true }).click();
      await expectDashboardFits(page);
      await expectOverviewContained(page.locator('.ol-overview-period-dates'));
      await page.getByRole('button', { name: 'Last 7 days', exact: true }).click();
      await page.getByRole('button', { name: 'Count', exact: true }).click();
      if ((size.width === 1400 && scale === 'large') || (size.width === 980 && scale === 'large')) {
        await page.screenshot({
          path: testInfo.outputPath(
            `acceptance-overview-${size.width}x${size.height}-${scale}.png`,
          ),
        });
      }

      const recent = page.locator('.ol-overview-recent');
      await recent.hover();
      await page.mouse.wheel(0, 160);
      await expect.poll(() => recent.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
      await expectDashboardFits(page);
      await page.getByRole('button', { name: 'Show top apps' }).click();
      const apps = page.locator('.ol-gha-app-list');
      await expect(apps).toBeVisible();
      const summary = page.locator('.ol-overview-year');
      if (await summary.evaluate((node) => getComputedStyle(node).position === 'static')) {
        const summaryBox = (await summary.boundingBox())!;
        const panelBox = (await page.locator('.ol-gha-panel').boundingBox())!;
        expect(panelBox.y).toBeGreaterThanOrEqual(summaryBox.y + summaryBox.height);
      }
      const overflow = await apps.evaluate((node) => node.scrollHeight - node.clientHeight);
      if (size.width === 980 && scale === 'large') expect(overflow).toBeGreaterThan(0);
      if (overflow > 0) {
        await apps.focus();
        await page.keyboard.press('End');
        await expect.poll(() => apps.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
      }
      await expect(apps.getByText('Application 5', { exact: true })).toBeInViewport();
      if (size.width === 980 && scale === 'large') {
        await page.screenshot({
          path: testInfo.outputPath('acceptance-overview-long-lists-expanded.png'),
        });
      }
      await page.getByRole('button', { name: 'Hide top apps' }).click();
      await expectDashboardFits(page);
    });
  }
}

for (const state of [
  'disabled',
  'activity-error',
  'history-error',
  'apps-error',
  'unsupported',
  'service-error',
]) {
  test(`the native dashboard keeps ${state} visible in the minimum window`, async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize({ width: 980, height: 640 });
    await page.goto(
      `${fixtureOrigin}/e2e/fixtures/native-overview.html?dashboard&scale=large&state=${state}`,
    );
    if (state === 'disabled' || state === 'activity-error') {
      await expect(page.locator('.ol-overview-heatmap')).toHaveCount(0);
    } else {
      await expect(page.locator('.ol-gha')).toBeVisible();
    }
    if (state === 'activity-error' || state === 'history-error') {
      const card = page.locator(
        state === 'activity-error' ? '.ol-overview-period' : '.ol-overview-recent',
      );
      await expectOverviewContained(card.getByRole('button', { name: 'Retry' }));
    }
    if (state === 'apps-error' || state === 'unsupported') {
      await expectOverviewContained(page.locator('.ol-overview-apps-status'));
      await expect(page.locator('.ol-gha-toggle')).toHaveCount(0);
    }
    if (state === 'service-error') {
      await expectOverviewContained(page.locator('.ol-overview-service-status'));
    }
    await expectDashboardFits(page);
  });
}

for (const locale of ['de', 'zh-CN']) {
  test(`the compact annual summary retains all ${locale} labels and long totals`, async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.setViewportSize({ width: 980, height: 640 });
    await page.goto(
      `${fixtureOrigin}/e2e/fixtures/native-overview.html?dashboard&scale=large&locale=${locale}`,
    );
    await expect(page.locator('.ol-gha-heading')).toContainText('987654321');
    await expectDashboardFits(page);
    await expectOverviewContained(page.locator('.ol-overview-period-dates'));
    for (const label of await page.locator('.ol-overview-year-label').all()) {
      expect(
        await label.evaluate((node) => node.scrollWidth - node.clientWidth),
      ).toBeLessThanOrEqual(1);
    }
  });
}
