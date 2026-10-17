// Production-browser instrumentation only; never label this native app launch time.
import { preview } from 'vite';
import { chromium } from '@playwright/test';
const runs = Number(process.argv[2] ?? 5);
if (!Number.isInteger(runs) || runs < 1 || runs > 20) throw new Error('Use 1–20 samples');
const server = await preview({ preview: { host: '127.0.0.1', port: 5341, strictPort: true } });
let browser;
try {
  browser = await chromium.launch();
  const samples = [];
  for (let i = 0; i < runs; i++) {
    const context = await browser.newContext({
      locale: 'zh-CN',
      colorScheme: 'dark',
      viewport: { width: 1300, height: 835 },
    });
    const page = await context.newPage();
    await page.goto('http://127.0.0.1:5341');
    await page.waitForFunction(
      () => performance.getEntriesByName('openless:interactive').length === 1,
    );
    samples.push(
      await page.evaluate(() => ({
        marks: Object.fromEntries(
          performance
            .getEntriesByType('mark')
            .filter((e) => e.name.startsWith('openless:'))
            .map((e) => [e.name.slice(9), Math.round(e.startTime)]),
        ),
        paints: Object.fromEntries(
          performance.getEntriesByType('paint').map((e) => [e.name, Math.round(e.startTime)]),
        ),
        loadedResources: performance.getEntriesByType('resource').length,
      })),
    );
    await context.close();
  }
  console.log(
    JSON.stringify(
      {
        scope:
          'Chromium headless, production assets, fresh browser contexts, mock backend; milliseconds from navigationStart. Not a native launch benchmark.',
        samples,
        medianInteractiveMs: samples.map((s) => s.marks.interactive).sort((a, b) => a - b)[
          Math.floor(runs / 2)
        ],
      },
      null,
      2,
    ),
  );
} finally {
  await browser?.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
