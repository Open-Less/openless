import { expect, test } from '@playwright/test';

for (const os of ['mac', 'win']) {
  test(`capsule display preference preserves supported modes across remount on ${os}`, async ({
    page,
  }) => {
    await page.goto(`/?os=${os}`);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    const control = dialog.getByRole('combobox', { name: '多屏胶囊', exact: true });
    const categoryNav = dialog.getByRole('navigation', { name: '设置分类' });
    const inputCategory = categoryNav.getByRole('button', { name: '录音与输入', exact: true });
    const appearanceCategory = dialog.getByRole('button', { name: '外观与语言', exact: true });
    const remountInputSection = async () => {
      await appearanceCategory.click();
      await inputCategory.click();
    };

    await expect(control).toContainText('鼠标所在屏幕');
    await control.click();
    if (os === 'win') {
      await expect(page.getByRole('option', { name: '仅主显示器', exact: true })).toBeVisible();
      await expect(
        page.getByRole('option', { name: '当前前台窗口所在显示器', exact: true }),
      ).toBeVisible();

      await page.getByRole('option', { name: '当前前台窗口所在显示器', exact: true }).click();
      await expect(control).toContainText('当前前台窗口所在显示器');
      await remountInputSection();
      await expect(control).toContainText('当前前台窗口所在显示器');

      await control.click();
      await page.getByRole('option', { name: '仅主显示器', exact: true }).click();
      await expect(control).toContainText('仅主显示器');
      await remountInputSection();
      await expect(control).toContainText('仅主显示器');

      const expectRestartWarning = async (mode: string) => {
        await control.click();
        await page.getByRole('option', { name: '所有屏幕显示', exact: true }).click();
        await control.click();
        await page.getByRole('option', { name: mode, exact: true }).click();
        await expect(
          page.getByRole('dialog', { name: '重启 OpenLess 以应用显示模式' }),
        ).toBeVisible();
        await page
          .getByRole('dialog', { name: '重启 OpenLess 以应用显示模式' })
          .getByRole('button', { name: '取消', exact: true })
          .click();
        await expect(
          page.getByRole('dialog', { name: '重启 OpenLess 以应用显示模式' }),
        ).toBeHidden();
      };
      await expectRestartWarning('当前前台窗口所在显示器');
      await expectRestartWarning('仅主显示器');
    } else {
      await expect(page.getByRole('option', { name: '仅主显示器', exact: true })).toHaveCount(0);
      await expect(
        page.getByRole('option', { name: '当前前台窗口所在显示器', exact: true }),
      ).toHaveCount(0);
    }

    if (os === 'win') {
      await control.click();
      await page.getByRole('option', { name: '所有屏幕显示', exact: true }).click();
    } else {
      await page.getByRole('option', { name: '所有屏幕显示', exact: true }).click();
    }
    await expect(control).toContainText('所有屏幕显示');
    await remountInputSection();
    await expect(control).toContainText('所有屏幕显示');

    await control.click();
    await page.getByRole('option', { name: '鼠标所在屏幕', exact: true }).click();
    await expect(control).toContainText('鼠标所在屏幕');
    if (os === 'win') {
      const restartDialog = page.getByRole('dialog', {
        name: '重启 OpenLess 以应用显示模式',
      });
      await expect(restartDialog).toBeVisible();
      await restartDialog.getByRole('button', { name: '现在重启', exact: true }).click();
      await expect(restartDialog).toBeHidden();
    }
  });
}
