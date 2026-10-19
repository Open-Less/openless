import { expect, test } from '@playwright/test';

for (const os of ['mac', 'win']) {
  test(`capsule display preference offers both modes and survives remount on ${os}`, async ({ page }) => {
    await page.goto(`/?os=${os}`);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    const control = dialog.getByRole('combobox', { name: '多屏胶囊', exact: true });
    await expect(control).toContainText('鼠标所在屏幕');
    await control.click();
    await page.getByRole('option', { name: '所有屏幕显示', exact: true }).click();
    await expect(control).toContainText('所有屏幕显示');
    await dialog.getByRole('button', { name: '外观与语言', exact: true }).click();
    await dialog.getByRole('navigation', { name: '设置分类' }).getByRole('button', { name: '录音与输入', exact: true }).click();
    await expect(control).toContainText('所有屏幕显示');
    await control.click();
    await page.getByRole('option', { name: '鼠标所在屏幕', exact: true }).click();
    await expect(control).toContainText('鼠标所在屏幕');
  });
}
