import { defineConfig } from '@playwright/test';
import path from 'node:path';
import base from './playwright.config';

const evidence = path.join(process.env.RUNNER_TEMP!, 'openless-dev-evidence');
export default defineConfig({
  ...base,
  testMatch: ['ask-window.spec.ts', 'history-learning.spec.ts'],
  grep: /manual Ask reference follows the new session and revocation clears its answer$|original page reference invalidation closes an open preview and permits a fresh Ask$|history learning uses a frozen source and releases it on close$/,
  projects: [{ name: 'chromium', use: { browserName: 'chromium' } }],
  // These tests import real source modules through the existing isolated fixture server.
  webServer: Array.isArray(base.webServer) ? [base.webServer[1]] : base.webServer,
  outputDir: path.join(evidence, 'ui'),
  reporter: [['list'], ['json', { outputFile: path.join(evidence, 'ui-results.json') }]],
  use: { ...base.use, screenshot: 'on', trace: 'retain-on-failure' },
});
