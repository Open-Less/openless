import { expect, test } from '@playwright/test';
import type { SettingsStatusFixture } from './fixtures/settings-status';
import { fixtureOrigin } from './support/server';

declare global {
  interface Window {
    settingsStatusFixture: SettingsStatusFixture;
  }
}

test.use({ baseURL: fixtureOrigin, locale: 'en-US' });

test('experimental download cancels only from its owning Settings page or outside a modal', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/settings-status.html?download');
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.experimentalReads()))
    .toBeGreaterThan(0);
  await page.evaluate(() => {
    const { setExperimentalStatus } = window.settingsStatusFixture;
    setExperimentalStatus('text-stream', { enabled: true, phase: 'downloading' });
    setExperimentalStatus('sense-voice', { enabled: true, phase: 'preparing' });
  });
  const progress = (model: string) =>
    page.locator(`[data-notification-key="download:experimental:${model}"][data-type="loading"]`);
  const result = (model: string) =>
    page.locator(`[data-notification-key="download:experimental:${model}"]:not([data-type="loading"])`);
  const textStream = progress('text-stream');
  const senseVoice = progress('sense-voice');
  const cancel = textStream.getByRole('button', { name: 'Cancel download', exact: true });
  await expect(textStream.getByRole('status')).toHaveText('Downloading TextStream');
  const bar = textStream.getByRole('progressbar');
  await expect(bar).toBeVisible();
  await expect(bar).not.toHaveAttribute('aria-valuenow');
  await expect(senseVoice.getByRole('status')).toHaveText('Preparing SenseVoice Small');

  await page.evaluate(() => {
    window.settingsStatusFixture.setExperimentalStatus('text-stream', {
      downloadedBytes: 0,
      totalBytes: 2500,
    });
  });
  await expect(textStream.getByRole('status')).toHaveText('Downloading TextStream · 0%');
  await page.evaluate(() => {
    window.settingsStatusFixture.setExperimentalStatus('text-stream', {
      downloadedBytes: 625,
      totalBytes: 2500,
    });
  });
  await expect(bar).toHaveAttribute('aria-valuenow', '25');
  await expect(textStream.getByRole('status')).toHaveText('Downloading TextStream · 25%');
  await expect(textStream.locator('[data-slot="toast-description"]')).toHaveCount(0);
  await expect(cancel).toBeEnabled();
  await expect(cancel).toHaveAttribute('title', 'Cancel the download of TextStream');

  await page.getByRole('button', { name: 'Open download modal' }).click();
  await expect(cancel).toBeDisabled();
  await page.getByRole('button', { name: 'Open local models' }).click();
  await expect(cancel).toBeEnabled();
  await page.getByRole('button', { name: 'Leave local models' }).click();
  await expect(cancel).toBeDisabled();
  await page.getByRole('button', { name: 'Open local models' }).click();
  await page.getByRole('button', { name: 'Open unrelated dialog' }).click();
  await expect(cancel).toBeDisabled();
  await page.keyboard.press('Escape');
  await expect(cancel).toBeEnabled();
  await cancel.click();
  await expect(cancel).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.experimentalToggles))
    .toEqual([{ model: 'text-stream', enabled: false }]);

  const reads = await page.evaluate(() => {
    const fixture = window.settingsStatusFixture;
    // A byte event and a status read can both complete while the disable command is in flight.
    void fixture.emitDownload('experimental-textstream-qwen3', 'finished');
    fixture.setExperimentalStatus('text-stream', { phase: 'ready', channelId: 'stale' });
    fixture.setExperimentalStatus('sense-voice', { phase: 'failed', error: 'test failure' });
    return fixture.experimentalReads();
  });
  await expect
    .poll(() => page.evaluate(() => window.settingsStatusFixture.experimentalReads()))
    .toBeGreaterThan(reads);
  await expect(result('sense-voice')).toHaveAttribute('data-type', 'warning');
  await expect(textStream).toBeVisible();
  await expect(result('text-stream')).toHaveCount(0);
  await page.evaluate(() => window.settingsStatusFixture.finishExperimentalToggle());
  await expect(textStream).toHaveCount(0);
  await expect(result('text-stream')).toHaveCount(0);

  await page.evaluate(() => {
    window.settingsStatusFixture.setExperimentalStatus('sense-voice', {
      phase: 'preparing',
      error: null,
    });
  });
  await expect(senseVoice).toBeVisible();
  await expect(result('sense-voice')).toHaveCount(0);
  await page.evaluate(() => {
    window.settingsStatusFixture.setExperimentalStatus('sense-voice', {
      phase: 'ready',
      channelId: 'sense-voice',
    });
  });
  await expect(result('sense-voice')).toHaveAttribute('data-type', 'success');
  await expect(result('sense-voice')).toHaveText('SenseVoice Small is ready');
  await expect(result('sense-voice').getByRole('progressbar')).toHaveCount(0);
  await expect(senseVoice).toHaveCount(0);
});

test('download labels stay readable through rapid percentages and one bounded phase transition', async ({ page }, info) => {
  await page.goto('/e2e/fixtures/settings-status.html?download&inlineDownloadStatus');
  await expect.poll(() => page.evaluate(() => window.settingsStatusFixture.experimentalReads())).toBeGreaterThan(0);
  const job = page.locator('[data-notification-key="download:experimental:embedding-gemma"]');
  const row = page.locator('[data-experimental-model="embedding-gemma"]');
  const change = (downloadedBytes: number) => page.evaluate(async (bytes) => {
    const fixture = window.settingsStatusFixture;
    fixture.setExperimentalStatus('embedding-gemma', {
      enabled: true, phase: 'downloading', totalBytes: 1000, downloadedBytes: bytes,
    });
    await fixture.refreshExperimental();
  }, downloadedBytes);
  await change(0);
  const status = job.getByRole('status');
  await expect(status).toHaveText('Downloading Embedding Gemma · 0%');
  await expect(row.getByRole('status')).toContainText('0%');
  await expect(job).toHaveCSS('width', '356px');
  const model = job.locator('.ol-download-model-name');
  const expectOneLine = async () => {
    const offset = await job.evaluate((element) => {
      const model = element.querySelector('.ol-download-model-name')!;
      const percent = element.querySelector('.ol-download-percent')!;
      return Math.abs(percent.getBoundingClientRect().y - model.getBoundingClientRect().y);
    });
    expect(offset).toBeLessThan(2);
  };
  await expectOneLine();
  const original = await model.boundingBox();
  for (const selector of ['.ol-download-model-name', '.ol-download-phase-text', '.ol-notification-body', '[role=progressbar]']) {
    await job.locator(selector).first().evaluate((node) => node.setAttribute('data-same-job', 'true'));
  }
  await job.screenshot({ path: info.outputPath('after-download-zero.png') });
  const cancel = job.getByRole('button', { name: 'Cancel download', exact: true });
  const cancelBefore = (await cancel.boundingBox())!;

  for (const [bytes, text] of [[3, '<1%'], [10, '1%'], [30, '3%']] as const) {
    await change(bytes);
    await expect(status).toHaveText(`Downloading Embedding Gemma · ${text}`);
    await expect(row.getByRole('status')).toContainText(text);
  }
  await page.waitForFunction(() =>
    Array.from(document.querySelectorAll('.ol-download-digits')).every(
      (node) => node.getAnimations({ subtree: true }).length === 0,
    ),
  );
  await expectOneLine();
  await job.screenshot({ path: info.outputPath('after-download-three-percent.png') });
  await row.getByRole('status').screenshot({ path: info.outputPath('after-download-inline-three-percent.png') });

  await page.evaluate(async () => {
    const fixture = window.settingsStatusFixture;
    for (let percent = 4; percent <= 30; percent++) {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      fixture.setExperimentalStatus('embedding-gemma', { downloadedBytes: percent * 10 });
      await fixture.refreshExperimental();
      const node = document.querySelector('.ol-notification-message .ol-download-phase-text');
      if (!node?.textContent?.includes('Downloading') || getComputedStyle(node).opacity !== '1')
        throw new Error('A byte update hid the fixed download label');
    }
  });
  await expect(status).toHaveText('Downloading Embedding Gemma · 30%');
  await expect(row.getByRole('status')).toContainText('30%');
  await expectOneLine();
  const cancelAfter = (await cancel.boundingBox())!;
  expect(Math.abs(cancelBefore.x - cancelAfter.x)).toBeLessThan(1);
  expect(cancelAfter.width).toBe(cancelBefore.width);
  expect(await job.locator('[data-same-job]').count()).toBe(4);
  expect(Math.abs((await model.boundingBox())!.x - original!.x)).toBeLessThan(1);

  const phase = await page.evaluate(async () => {
    const fixture = window.settingsStatusFixture;
    const start = performance.now();
    fixture.setExperimentalStatus('embedding-gemma', { phase: 'preparing' });
    await fixture.refreshExperimental();
    let peakBlur = 0;
    while (performance.now() - start < 500) {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      const node = document.querySelector('.ol-notification-message .ol-download-phase-text');
      if (!node) continue;
      const { filter, opacity } = getComputedStyle(node);
      const blur = Number(filter.match(/blur\(([\d.]+)px\)/)?.[1] ?? 0);
      peakBlur = Math.max(peakBlur, blur);
      if (node.textContent?.includes('Preparing') && Number(opacity) >= 0.999 && blur <= 0.01)
        return { peakBlur, elapsed: performance.now() - start };
    }
    throw new Error('Phase wording did not settle');
  });
  expect(phase.peakBlur).toBeGreaterThan(0);
  expect(phase.peakBlur).toBeLessThanOrEqual(1.21);
  expect(phase.elapsed).toBeLessThanOrEqual(300);
  console.log('Download phase measurement', phase);
  await expect(status).toHaveText('Preparing Embedding Gemma');
  await expect(model).toHaveAttribute('data-same-job', 'true');
  await job.screenshot({ path: info.outputPath('after-download-phase-change.png') });

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(async () => {
    const fixture = window.settingsStatusFixture;
    fixture.setExperimentalStatus('embedding-gemma', { phase: 'ready', channelId: 'fixture-channel' });
    await fixture.refreshExperimental();
  });
  await expect(job).toHaveAttribute('data-type', 'success');
  await expect(status).toHaveText('Embedding Gemma is ready');
  await expect(job.locator('.ol-notification-body')).toHaveAttribute('data-same-job', 'true');
  await expect(job.getByRole('progressbar')).toHaveCount(0);
});

test('a long download label can wrap and keeps its percentage and cancel control readable', async ({
  page,
}, info) => {
  await page.goto('/e2e/fixtures/settings-status.html?download');
  await expect.poll(() => page.evaluate(() => window.settingsStatusFixture.hasDownloadListener())).toBe(true);
  const name = 'Multilingual speech recognition large model for offline transcription';
  await page.evaluate((model) => window.settingsStatusFixture.emitDownload(model, 'started'), name);
  const job = page.getByRole('status').filter({ hasText: name });
  await expect(job).toHaveText(`Downloading ${name} · 1%`);
  const panel = page.locator('[data-type="loading"]').filter({ has: job });
  const percent = panel.locator('.ol-download-percent');
  const cancel = panel.getByRole('button', { name: 'Cancel download', exact: true });
  await expect(percent).toBeVisible();
  await expect(cancel).toBeEnabled();
  const lines = await panel.locator('.ol-download-model-name').evaluate((element) => {
    const range = document.createRange();
    range.selectNodeContents(element);
    return range.getClientRects().length;
  });
  expect(lines).toBeGreaterThan(1);
  const percentBounds = (await percent.boundingBox())!;
  expect(percentBounds.x + percentBounds.width).toBeLessThan((await cancel.boundingBox())!.x);
  await panel.screenshot({ path: info.outputPath('after-download-long-label.png') });
});
