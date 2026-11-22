import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

const fixture = fixtureOrigin + '/e2e/fixtures/updates-interactions.html';
test.use({ locale: 'en-US', reducedMotion: 'reduce' });

test('cancel settles the update choice and releases the resource without installing', async ({
  page,
}) => {
  await page.goto(fixture);
  const button = page.getByRole('button', { name: 'Manual check' });
  await button.click();
  const prompt = page.getByRole('dialog', { name: 'Switch to Stable' });
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText('2.0.1-beta.1');
  await page.keyboard.press('Escape');
  await expect(prompt).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).updateFixture.writes
          .filter((write: { command: string }) => /^(plugin:|restart_app)/.test(write.command))
          .map((write: { command: string; args: unknown }) => [write.command, write.args]),
      ),
    )
    .toEqual([['plugin:resources|close', { rid: 19 }]]);
});

test('confirmed installation survives route disposal and restart waits for a user choice', async ({
  page,
}) => {
  await page.goto(fixture);
  await page.getByRole('button', { name: 'Manual check' }).click();
  await page
    .getByRole('dialog', { name: 'Switch to Stable' })
    .getByRole('button', { name: 'Update now' })
    .click();
  const progress = page.getByRole('dialog', { name: 'Downloading update' });
  await expect(progress).toBeVisible();
  await expect(progress.getByRole('progressbar')).toBeVisible();
  await page.evaluate(() => (window as any).updateFixture.hideManual());
  await expect(page.getByRole('button', { name: 'Manual check', includeHidden: true })).toHaveCount(
    0,
  );
  await expect(progress).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(progress).toBeVisible();
  await page.evaluate(() => (window as any).updateFixture.finishDownload());
  const done = page.getByRole('dialog', { name: 'Update ready' });
  await expect(done).toBeVisible();
  expect(
    await page.evaluate(() =>
      (window as any).updateFixture.writes.some(
        (write: { command: string }) => write.command === 'restart_app',
      ),
    ),
  ).toBe(false);
  await done.getByRole('button', { name: 'Restart now' }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as any).updateFixture.writes.filter(
            (write: { command: string }) => write.command === 'restart_app',
          ).length,
      ),
    )
    .toBe(1);
});

test('a background candidate waits for an existing window decision', async ({ page }) => {
  await page.goto(fixture);
  await page.getByRole('button', { name: 'Other decision' }).click();
  const existing = page.getByRole('dialog', { name: 'Existing decision' });
  await expect(existing).toBeVisible();
  await page.evaluate(() => (window as any).updateFixture.checkInBackground());
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).updateFixture.writes.some(
          (write: { command: string }) => write.command === 'app_check_update_with_channel',
        ),
      ),
    )
    .toBe(true);
  await expect(existing).toBeVisible();
  await expect(page.getByRole('dialog', { name: 'Switch to Stable' })).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(page.getByRole('dialog', { name: 'Switch to Stable' })).toBeVisible();
});

test('download failure retains an explicit manual fallback', async ({ page }) => {
  await page.goto(fixture);
  await page.evaluate(() => {
    (window as any).updateFixture.failDownload = true;
  });
  await page.getByRole('button', { name: 'Manual check' }).click();
  await page.getByRole('button', { name: 'Update now' }).click();
  const failure = page.getByRole('dialog', { name: 'Update failed' });
  await expect(failure).toContainText('network unavailable');
  expect(
    await page.evaluate(() =>
      (window as any).updateFixture.writes.some(
        (write: { command: string }) => write.command === 'plugin:updater|install',
      ),
    ),
  ).toBe(false);
  await failure.getByRole('button', { name: 'Download manually' }).click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (window as any).updateFixture.writes.some(
          (write: { command: string; args?: { path?: string } }) =>
            write.command === 'plugin:shell|open' &&
            write.args?.path === 'https://github.com/Open-Less/openless/releases',
        ),
      ),
    )
    .toBe(true);
});
