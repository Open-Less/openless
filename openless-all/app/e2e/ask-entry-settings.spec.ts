import { expect, test, type Page } from '@playwright/test';
import type { AskEntrySettingsFixture } from './fixtures/ask-entry-settings';
import { settleNavigation } from './support/navigation';
import { fixtureOrigin } from './support/server';

declare global {
  interface Window {
    askEntrySettingsFixture: AskEntrySettingsFixture;
  }
}

const fixture = `${fixtureOrigin}/e2e/fixtures/ask-entry-settings.html`;

async function openAskSettings(page: Page) {
  await page.locator('[data-ol-general-entry="askEntry"]').click();
  await settleNavigation(page);
  await expect(page.getByRole('heading', { name: 'Ask Anything', exact: true })).toBeVisible();
}

test('defaults, full commits, exclusive Beta modes, and reload', async ({ page }) => {
  await page.goto(fixture);
  await openAskSettings(page);
  const autoVoice = page.getByRole('switch', { name: 'Auto-start voice', exact: true });
  const separate = page.getByRole('switch', { name: 'Separate model (Beta)', exact: true });
  const mainTool = page.getByRole('switch', { name: 'Main model tool (Beta)', exact: true });
  const currentMode = page.locator('span[aria-live="polite"]');
  await expect(autoVoice).toBeChecked();
  await expect(separate).toBeChecked();
  await expect(mainTool).not.toBeChecked();
  await expect(currentMode).toHaveText('Separate model');
  await expect(page.getByRole('switch', { name: 'Save history' })).toBeChecked();
  await expect(
    page.getByRole('switch', { name: 'Share desktop app names with Ask' }),
  ).toBeChecked();

  await autoVoice.click();
  await expect(autoVoice).not.toBeChecked();
  await autoVoice.click();
  await expect(autoVoice).toBeChecked();
  await autoVoice.click();
  await expect(autoVoice).not.toBeChecked();
  await separate.click();
  await expect(separate).not.toBeChecked();
  await expect(mainTool).not.toBeChecked();
  await expect(currentMode).toHaveText('Semantic hybrid');
  await mainTool.click();
  await expect(mainTool).toBeChecked();
  await expect(separate).not.toBeChecked();
  expect(await page.evaluate(() => window.askEntrySettingsFixture.writes)).toEqual([
    { autoVoiceOnOpen: false, intentMode: 'separateModel' },
    { autoVoiceOnOpen: true, intentMode: 'separateModel' },
    { autoVoiceOnOpen: false, intentMode: 'separateModel' },
    { autoVoiceOnOpen: false, intentMode: 'semanticHybrid' },
    { autoVoiceOnOpen: false, intentMode: 'mainModelTool' },
  ]);

  await page.reload();
  await openAskSettings(page);
  await expect(autoVoice).not.toBeChecked();
  await expect(mainTool).toBeChecked();
  await expect(separate).not.toBeChecked();
  await mainTool.click();
  await expect(currentMode).toHaveText('Semantic hybrid');
  await separate.click();
  await expect(separate).toBeChecked();
  await separate.click();
  await expect(currentMode).toHaveText('Semantic hybrid');
});

test('keyboard can reach hints and switches without showing inline explanations', async ({
  page,
}) => {
  await page.goto(fixture);
  await openAskSettings(page);
  const voiceLabel = page.getByRole('button', { name: 'Auto-start voice', exact: true });
  const voiceSwitch = page.getByRole('switch', { name: 'Auto-start voice', exact: true });
  const tooltip = page.getByRole('tooltip');
  await voiceLabel.hover();
  await expect(tooltip).toContainText('recording key plus Space');
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
  await voiceSwitch.focus();
  await page.keyboard.press('Shift+Tab');
  await expect(voiceLabel).toBeFocused();
  await expect(tooltip).toBeVisible();
  await expect(voiceLabel).toHaveAttribute('aria-describedby', (await tooltip.getAttribute('id'))!);
  await page.keyboard.press('Escape');
  await expect(tooltip).toBeHidden();
  await expect(page.getByRole('heading', { name: 'Ask Anything', exact: true })).toBeVisible();
  await page.keyboard.press('Tab');
  await expect(voiceSwitch).toBeFocused();
  await page.keyboard.press('Space');
  await expect(voiceSwitch).not.toBeChecked();
  await expect(page.locator('.ol-setting-row-description:visible')).toHaveCount(0);
});

test('a failed pending save reports a notice and preserves committed values', async ({ page }) => {
  await page.goto(`${fixture}?defer`);
  await openAskSettings(page);
  const voice = page.getByRole('switch', { name: 'Auto-start voice', exact: true });
  const separate = page.getByRole('switch', { name: 'Separate model (Beta)', exact: true });
  const mainTool = page.getByRole('switch', { name: 'Main model tool (Beta)', exact: true });
  await voice.click();
  await expect(voice).toBeChecked();
  await expect(separate).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.askEntrySettingsFixture.hasPendingWrite()))
    .toBe(true);
  await page.evaluate(() => window.askEntrySettingsFixture.finish(true));
  await expect(page.getByRole('alert')).toContainText('Could not save Ask settings');
  await expect(voice).toBeChecked();
  await expect(separate).toBeEnabled();
  await mainTool.click();
  await expect
    .poll(() => page.evaluate(() => window.askEntrySettingsFixture.hasPendingWrite()))
    .toBe(true);
  await page.getByRole('button', { name: 'Back', exact: true }).click();
  await openAskSettings(page);
  await expect(mainTool).toHaveCount(0);
  await page.evaluate(() => window.askEntrySettingsFixture.finish(false));
  await expect(mainTool).toBeChecked();
});

test('a native read failure requires explicit, committed recovery', async ({ page }) => {
  await page.goto(`${fixture}?failRead&defer`);
  await openAskSettings(page);
  const restore = page.getByRole('button', { name: 'Restore Defaults', exact: true });
  const autoVoice = page.getByRole('switch', { name: 'Auto-start voice', exact: true });
  await expect(page.getByRole('alert')).toContainText('Could not load Ask settings');
  await expect(autoVoice).toHaveCount(0);
  expect(await page.evaluate(() => window.askEntrySettingsFixture.writes)).toEqual([]);

  await restore.click();
  await expect(restore).toBeDisabled();
  await expect
    .poll(() => page.evaluate(() => window.askEntrySettingsFixture.hasPendingWrite()))
    .toBe(true);
  await page.evaluate(() => window.askEntrySettingsFixture.finish(true));
  await expect(page.getByRole('alert')).toContainText('Could not save Ask settings');
  await expect(autoVoice).toHaveCount(0);
  await expect(restore).toBeEnabled();

  await restore.click();
  await expect
    .poll(() => page.evaluate(() => window.askEntrySettingsFixture.hasPendingWrite()))
    .toBe(true);
  await expect(autoVoice).toHaveCount(0);
  await page.evaluate(() => window.askEntrySettingsFixture.finish(false));
  await expect(autoVoice).toBeChecked();
  await expect(page.locator('span[aria-live="polite"]')).toHaveText('Separate model');
  await expect(restore).toHaveCount(0);
  await expect(page.getByRole('alert')).toHaveCount(0);
  expect(await page.evaluate(() => window.askEntrySettingsFixture.writes)).toEqual([
    { autoVoiceOnOpen: true, intentMode: 'separateModel' },
    { autoVoiceOnOpen: true, intentMode: 'separateModel' },
  ]);

  await page.reload();
  await openAskSettings(page);
  await expect(autoVoice).toBeChecked();
  await expect(restore).toHaveCount(0);
});
