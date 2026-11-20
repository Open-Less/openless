import { fixtureOrigin } from './support/server';
import { expect, test } from '@playwright/test';

const origin = fixtureOrigin;
const fixture = `${origin}/e2e/fixtures/startup-gate.html`;

test.beforeEach(async ({ page }) => {
  await page.route(
    /\/src\/(?:app\/shell\/FloatingShell|features\/(?:onboarding\/Onboarding|downloads\/GlobalDownloadProgress))\.tsx(?:\?.*)?$/,
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
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.presentations))
    .toEqual([expect.objectContaining({ recovering: true })]);
});

for (const theme of ['light', 'dark'] as const) {
  test(`macOS first display waits for the shell with the selected ${theme} theme`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: theme === 'light' ? 'dark' : 'light' });
    await page.goto(`${fixture}?os=mac&credentials=configured&nativeTheme=${theme}`);
    const app = page.getByRole('heading', { name: 'Startup reached the app' });
    await expect(app).toBeVisible();
    const presentations = () => page.evaluate(() => (window as any).startupFixture.presentations);
    expect(await presentations()).toEqual([]);
    await page.evaluate(() => (window as any).startupFixture.releaseTheme());
    await expect
      .poll(presentations)
      .toEqual([{ theme, heading: 'Startup reached the app', recovering: false }]);
    await expect(page.locator('.ol-startup')).toHaveCount(0);
  });
}

test('Android Back dismisses Voice Edit safely and ignores legacy Ask events', async ({
  page,
}) => {
  await page.goto(`${fixture}?os=android`);
  const app = page.getByRole('heading', { name: 'Startup reached the app' });
  const voice = page.locator('.ol-voice-edit-window');
  await expect(app).toBeVisible();
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.listening('voice-edit:show')))
    .toBe(true);
  await page.evaluate(() => (window as any).startupFixture.showVoice('old-draft'));
  await expect(voice).toBeVisible();
  await page.evaluate(() => (window as any).startupFixture.emit('qa:state', { kind: 'idle' }));
  await expect(voice.getByText('old-draft').first()).toBeVisible();
  expect(await page.evaluate(() => (window as any).startupFixture.pendingCancels())).toEqual([]);
  await page.evaluate(() => history.back());
  await expect
    .poll(() => page.evaluate(() => (window as any).startupFixture.pendingCancels()))
    .toEqual(['old-draft']);
  await page.evaluate(() => (window as any).startupFixture.showVoice('new-draft'));
  await expect(voice.getByText('new-draft').first()).toBeVisible();
  await page.evaluate(() => (window as any).startupFixture.finishCancel(true));
  await expect(voice.getByText('new-draft').first()).toBeVisible();

  for (const succeed of [false, true]) {
    await page.evaluate(() => history.back());
    await expect
      .poll(() => page.evaluate(() => (window as any).startupFixture.pendingCancels()))
      .toEqual(['new-draft']);
    await expect(voice).toBeVisible();
    await page.evaluate((result) => (window as any).startupFixture.finishCancel(result), succeed);
    if (!succeed) {
      await expect(voice.getByRole('alert')).toHaveText('Microphone is still busy');
    }
  }
  await expect(voice).toHaveCount(0);
  await expect(app).toBeVisible();
  await page.evaluate(() =>
    (window as any).startupFixture.emit('qa:state', { kind: 'answer', sessionId: 'closed' }),
  );
  await expect(app).toBeVisible();
  await expect(page.locator('.ol-embedded-panel')).toHaveCount(0);
});
