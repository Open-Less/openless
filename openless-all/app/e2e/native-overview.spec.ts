import { expect, test } from '@playwright/test';
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
  await appReads.click();
  await expect(appReads).toHaveText('2');
  await page.getByRole('button', { name: 'Publish activity write' }).click();
  await expect(heading).toContainText('43 dictations');
  await appReads.click();
  await expect(appReads).toHaveText('2');
  await page.getByRole('button', { name: 'Complete old app read' }).click();
  await expect(chips).toContainText('43');
  await appReads.click();
  await expect(appReads).toHaveText('3');
});
