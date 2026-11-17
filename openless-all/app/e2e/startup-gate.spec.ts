import { expect, test } from '@playwright/test';

const origin = 'http://127.0.0.1:5326';
const fixture = `${origin}/e2e/fixtures/startup-gate.html`;

test.beforeEach(async ({ page }) => {
  await page.route(
    /\/src\/components\/(FloatingShell|Onboarding|GlobalDownloadProgress)\.tsx(?:\?.*)?$/,
    async (route) => {
      const response = await page.request.get(`${origin}/e2e/fixtures/startup-destinations.tsx`);
      await route.fulfill({ response });
    },
  );
});

for (const os of ['mac', 'win']) {
  test(`${os}: a rejected credential probe uses the existing startup fallback`, async ({
    page,
  }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`${fixture}?os=${os}&credentials=reject`);
    await expect(page.getByRole('heading', { name: 'Startup reached the app' })).toBeVisible();
    await expect(page.locator('.ol-startup')).toHaveCount(0);
    expect(errors).toEqual([]);
  });

  for (const late of ['resolve', 'reject']) {
    test(`${os}: a hung store cannot block startup or reroute the app after a late ${late}`, async ({
      page,
    }) => {
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      await page.goto(`${fixture}?os=${os}&credentials=hang`);
      await expect
        .poll(() =>
          page.evaluate(() => (window as any).startupFixture.calls.includes('get_credentials')),
        )
        .toBe(true);
      await expect(page.getByRole('heading', { name: 'Startup reached the app' })).toBeVisible();
      await page.evaluate((late) => (window as any).startupFixture[late](), late);
      await page.waitForTimeout(100);
      await expect(page.getByRole('heading', { name: 'Startup reached the app' })).toBeVisible();
      await expect(page.getByRole('heading', { name: 'Startup reached the guide' })).toHaveCount(0);
      expect(await page.evaluate(() => localStorage.getItem('ol.desktopOnboarding.v1'))).toBeNull();
      expect(errors).toEqual([]);
    });
  }

  test(`${os}: responsive credential status still selects the correct destination`, async ({
    page,
  }) => {
    for (const mode of ['configured', 'unconfigured']) {
      await page.goto(`${fixture}?os=${os}&credentials=${mode}`);
      await expect(
        page.getByRole('heading', {
          name: `Startup reached the ${mode === 'configured' ? 'app' : 'guide'}`,
        }),
      ).toBeVisible();
    }
  });
}

test('the optional-probe fallback never bypasses a failed backend contract handshake', async ({
  page,
}) => {
  await page.goto(`${fixture}?badHandshake=1&credentials=hang`);
  await expect(page.locator('.ol-startup [role="alert"]')).toBeVisible();
  expect(await page.evaluate(() => (window as any).startupFixture.calls)).not.toContain(
    'get_credentials',
  );
  await expect(page.getByRole('heading')).toHaveCount(0);
});
