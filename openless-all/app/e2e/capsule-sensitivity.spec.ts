import { expect, test } from '@playwright/test';

test('capsule style section features a vertical sensitivity regulator next to the preview', async ({
  page,
}, info) => {
  await page.goto('/?os=mac');

  // Open Settings dialog
  const settingsBtn = page.getByRole('button', { name: '设置', exact: true });
  await expect(settingsBtn).toBeVisible();
  await settingsBtn.click();
  const dialog = page.getByRole('dialog', { name: '设置', exact: true });
  await expect(dialog).toBeVisible();

  // Navigate to recording and input settings
  const recordingNav = dialog.getByRole('button', { name: /录音与输入|录音|Recording|入力/i });
  if (await recordingNav.isVisible()) {
    await recordingNav.click();
  }

  // The preview row should contain both the preview box and the vertical regulator side by side
  const previewRow = dialog.locator('.ol-capsule-preview-row');
  await expect(previewRow).toBeVisible();

  const preview = previewRow.locator('.ol-capsule-style-preview');
  await expect(preview).toBeVisible();

  const regulator = previewRow.locator('.ol-sensitivity-regulator');
  await expect(regulator).toBeVisible();
  await expect(regulator).toHaveAttribute('role', 'slider');
  await expect(regulator).toHaveAttribute('aria-valuemin', '0.5');
  await expect(regulator).toHaveAttribute('aria-valuemax', '2.5');
  await expect(regulator).toHaveAttribute('aria-valuenow', '1');

  // Test keyboard interaction: ArrowUp increases sensitivity by 0.1
  await regulator.focus();
  await page.keyboard.press('ArrowUp');
  await expect(regulator).toHaveAttribute('aria-valuenow', '1.1');
  await expect(regulator.locator('.ol-sensitivity-value')).toHaveText('1.1×');

  // Double-click resets to default 1.0×
  await regulator.dblclick();
  await expect(regulator).toHaveAttribute('aria-valuenow', '1');
  await expect(regulator.locator('.ol-sensitivity-value')).toHaveText('1.0×');

  // Capture screenshot of the settings capsule style row
  await page.screenshot({
    path: info.outputPath('capsule-sensitivity-regulator.png'),
    animations: 'disabled',
  });
});
