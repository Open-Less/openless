import { expect, test } from '@playwright/test';
import { zhCN } from '../src/i18n/zh-CN';
import { en } from '../src/i18n/en';

for (const [locale, strings] of [
  ['zh-CN', zhCN],
  ['en', en],
] as const) {
  test(`DeepSeek ASR uses a key without model controls in ${locale}`, async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.addInitScript((locale) => localStorage.setItem('ol.locale', locale), locale);
    await page.goto('/');
    await page.getByRole('button', { name: /设置|Settings/, exact: true }).click();
    await page.getByRole('button', { name: /AI 与模型|AI & models/, exact: true }).click();
    await page
      .locator('.ol-service-views')
      .getByRole('button', { name: /语音识别|Speech recognition/ })
      .click();
    await page.getByRole('button', { name: /添加渠道|Add channel/, exact: true }).click();
    const form = page.getByRole('dialog', { name: /添加渠道|Add channel/, exact: true });
    const provider = form.getByRole('combobox', {
      name: strings.settings.providers.providerLabel,
      exact: true,
    });
    await provider.click();
    await page.getByRole('option', { name: 'DeepSeek ASR', exact: true }).click();
    await expect(form).toContainText(strings.settings.providers.deepseekAsrNotice);
    await expect(
      form.getByRole('textbox', { name: strings.settings.providers.modelLabel, exact: true }),
    ).toHaveCount(0);
    await expect(
      form.getByRole('button', { name: strings.settings.providers.fetchModels, exact: true }),
    ).toHaveCount(0);
    await expect(
      form.getByRole('textbox', { name: strings.settings.providers.baseUrlLabel, exact: true }),
    ).toHaveValue('https://api.deepseek.com/v1');
    const key = form.getByLabel(strings.settings.providers.apiKeyLabel, { exact: true });
    await key.fill('sk-ui-fixture');
    await key.blur();
    await expect(key).toHaveAttribute('type', 'password');
    const verify = form.getByRole('button', {
      name: strings.settings.channels.verify,
      exact: true,
    });
    await verify.click();
    await expect(form).toContainText(strings.settings.providers.validateSuccessAudio);
    await page.screenshot({ path: `../../.context/deepseek-asr-${locale}.png` });
    await provider.click();
    await page
      .getByRole('option', { name: strings.settings.providers.presets.asrWhisper, exact: true })
      .click();
    await expect(
      form.getByRole('textbox', { name: strings.settings.providers.modelLabel, exact: true }),
    ).toBeVisible();
    await expect(
      form.getByRole('button', { name: strings.settings.providers.fetchModels, exact: true }),
    ).toBeVisible();
    expect(errors).toEqual([]);
  });
}
