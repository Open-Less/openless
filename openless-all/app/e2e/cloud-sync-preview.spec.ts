import { expect, test, type Page } from '@playwright/test';

async function openCloudSettings(page: Page, settings: string, cloud: string) {
  await page.getByRole('button', { name: settings, exact: true }).click();
  const modal = page.getByRole('dialog', { name: settings, exact: true });
  await modal.getByRole('button', { name: cloud, exact: true }).click();
  return modal;
}

test('the ordinary Chinese preview uses the real settings flow to set a sync password', async ({
  page,
}) => {
  await page.goto('/?os=mac&cloudSync=first-time&cloudSyncFailure=create');
  const settings = await openCloudSettings(page, '设置', '云服务');
  await expect(settings.getByText('云同步演示。', { exact: false })).toBeVisible();
  const enable = settings.getByRole('switch', { name: '启用加密同步' });
  const enableState = page.locator('.ol-cloud-sync [role="switch"]');
  await expect(enable).not.toBeChecked();
  await enable.click();
  const agreement = page.getByRole('dialog', { name: '云同步服务告知与协议草案' });
  const agree = agreement.getByRole('button', { name: '同意并继续' });
  await expect(agree).toBeDisabled();
  await agreement.getByRole('button', { name: '暂不同意' }).click();
  await expect(agreement).toHaveCount(0);
  await expect(enable).not.toBeChecked();
  await enable.click();
  await agreement.getByRole('checkbox').check();
  await agree.click();
  const form = page.getByRole('dialog', { name: '设置云同步密码' });
  await expect(form).toHaveAccessibleDescription(/创建成功后才会开启同步/);
  const submit = form.getByRole('button', { name: '确认并继续' });
  await expect(submit).toBeDisabled();
  const password = form.getByLabel('同步密码', { exact: true });
  const confirmation = form.getByLabel('再次输入同步密码', { exact: true });
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await password.fill('PreviewExample1');
    await confirmation.fill('PreviewExample1');
    await submit.click();
    if (attempt === 0) {
      await expect(form.getByRole('alert')).toBeVisible();
      await expect(password).toHaveValue('');
      await expect(enableState).toHaveAttribute('aria-checked', 'false');
    }
  }
  await expect(form).toHaveCount(0);
  await expect(enable).toBeChecked();
});

test.describe('English preview', () => {
  test.use({ locale: 'en-US' });

  test('shows the renewal immediately and resumes only after explicit agreement', async ({
    page,
  }) => {
    await page.goto('/?os=mac&cloudSync=reconsent&cloudSyncFailure=prepare');
    const terms = page.getByRole('dialog', { name: 'The cloud sync disclosure has changed' });
    await expect(terms.getByText(/Your previous confirmation will not carry over/)).toBeVisible();
    await terms.getByRole('button', { name: 'Not now' }).click();
    await expect(terms).toHaveCount(0);
    const settings = await openCloudSettings(page, 'Settings', 'Cloud services');
    const sync = settings.getByRole('button', { name: 'Sync now', exact: true });
    await expect(sync).toBeDisabled();
    await settings.getByRole('button', { name: 'The cloud sync disclosure has changed' }).click();
    await terms.getByRole('checkbox').check();
    const agree = terms.getByRole('button', { name: 'Agree to the update and continue' });
    await agree.click();
    await expect(terms.getByRole('alert')).toBeVisible();
    await agree.click();
    await expect(terms).toHaveCount(0);
    await expect(sync).toBeEnabled();
  });
});
