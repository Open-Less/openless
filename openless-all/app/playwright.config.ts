import { defineConfig } from '@playwright/test';
import { fixtureOrigin, fixturePort, previewOrigin, previewPort } from './e2e/support/server';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  retries: 0,
  workers: 1,
  timeout: 30_000,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: previewOrigin,
    viewport: { width: 1300, height: 835 },
    locale: 'zh-CN',
    colorScheme: 'dark',
    reducedMotion: 'no-preference',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    { name: 'chromium', use: { browserName: 'chromium' } },
    { name: 'webkit', use: { browserName: 'webkit' } },
  ],
  webServer: [
    {
      command: `npm run preview -- --host 127.0.0.1 --port ${previewPort} --strictPort`,
      url: previewOrigin,
      reuseExistingServer: !process.env.CI && !process.env.OPENLESS_E2E_PREVIEW_PORT,
    },
    // Isolated React fixtures use real source modules, without shipping test routes/hooks.
    {
      command: `npm run dev -- --host 127.0.0.1 --port ${fixturePort} --strictPort`,
      url: fixtureOrigin,
      reuseExistingServer: !process.env.CI && !process.env.OPENLESS_E2E_FIXTURE_PORT,
    },
  ],
});
