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
    page.locator(`[data-notification-key="download:experimental:${model}"]`);
  const result = (model: string) =>
    page.locator(`[data-notification-key="download-result:experimental:${model}"]`);
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
