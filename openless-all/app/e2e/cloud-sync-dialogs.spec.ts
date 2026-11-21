import { expect, test, type Locator, type Page } from '@playwright/test';
import { fixtureOrigin } from './support/server';

test.use({ baseURL: fixtureOrigin, locale: 'en-US', colorScheme: 'light' });

function attempts(page: Page) {
  return page.evaluate(() => (window as any).syncDialogFixture.attempts);
}
function finish(page: Page, failed = false) {
  return page.evaluate((error) => (window as any).syncDialogFixture.finish(error), failed);
}
async function dismissWhilePending(page: Page, dialog: Locator) {
  await page.keyboard.press('Escape');
  const box = await page.locator('.ol-app-viewport').boundingBox();
  await page.mouse.click(box!.x + 3, box!.y + 3);
  await expect(dialog).toBeVisible();
}

test('restore waits for the sync result and shows task contention above the review', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=review');
  await page.getByRole('button', { name: 'Sync now', exact: true }).click();
  const toast = page.locator('[data-notification-key="cloud-sync-operation"]');
  await expect(toast.getByRole('status')).toContainText('Syncing the latest cloud data');
  await expect(toast.getByRole('progressbar')).toHaveCount(0);
  await expect(toast).toHaveCSS('width', '356px');
  await page.evaluate(() => (window as any).syncDialogFixture.publishReview());
  const review = page.getByRole('dialog', { name: 'Review before restoring' });
  await expect(review).toHaveCount(0);
  await finish(page);
  await expect(review).toBeVisible();
  await expect(toast.getByRole('alert')).toHaveText('Review before restoring');
  await expect(toast).toHaveAttribute('data-severity', 'warning');
  await expect(toast).toHaveCSS('width', '356px');
  await review.getByRole('checkbox').check();
  const confirm = review.getByRole('button', { name: 'Confirm restore' });
  await confirm.click();
  await expect(toast.getByRole('alert')).toHaveText(
    'Another sync task is running. Wait or cancel it first.',
  );
  await expect(toast).toHaveAttribute('data-severity', 'warning');
  await expect(page.locator('[data-notification-key="cloud-sync-busy"]')).toHaveCount(0);
  await expect(review.getByRole('alert')).toHaveCount(0);
  await expect(review.getByRole('checkbox')).toBeChecked();
  await confirm.click();
  await expect(review).toHaveCount(0);
  await expect(toast.getByRole('status')).toBeVisible();
  expect(
    (await attempts(page)).map(({ command, args }: any) => [command, args?.previewId]),
  ).toEqual([
    ['cloud_sync_e2ee_sync_now', undefined],
    ['cloud_sync_e2ee_apply_restore', 'restore-preview'],
    ['cloud_sync_e2ee_apply_restore', 'restore-preview'],
  ]);
});

function syncRuns(page: Page) {
  return page.evaluate(() => (window as any).syncDialogFixture.syncRuns);
}

test('new syncs start after success failure and cancellation', async ({ page }) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=sync-lifecycle');
  const sync = page.getByRole('button', { name: 'Sync now', exact: true });
  const toast = page.locator('[data-notification-key="cloud-sync-operation"]');
  for (const [index, outcome] of (['success', 'failure', 'cancel', 'success'] as const).entries()) {
    await sync.click();
    await expect(toast.getByRole('status')).toContainText('Syncing the latest cloud data');
    await expect.poll(async () => (await syncRuns(page)).length).toBe(index + 1);
    await expect(sync).toBeDisabled();
    const capsule = await toast.elementHandle();
    if (outcome === 'cancel') await toast.getByRole('button', { name: 'Cancel sync' }).click();
    else await page.evaluate((outcome) => (window as any).syncDialogFixture.finishSync(outcome), outcome);
    await expect(sync).toBeEnabled();
    await expect(toast).not.toHaveAttribute('data-state', 'saving');
    expect(await capsule!.evaluate((element) => element.isConnected && !element.hasAttribute('aria-hidden'))).toBe(true);
    await expect(page.locator('.ol-cloud-sync [role="alert"]')).toHaveCount(0);
  }
  expect((await syncRuns(page))).toEqual(['sync-1', 'sync-2', 'sync-3', 'sync-4']);
});

test('renewal appears before settings, stays paused on dismissal, and waits for native confirmation', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=renew');
  const dialog = page.getByRole('dialog', { name: /disclosure has changed|updated disclosure/i });
  const confirm = dialog.getByRole('button', { name: 'Agree to the update and continue' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toHaveCSS('width', '512px');
  await expect(dialog.locator('.ol-dialog-header')).toHaveCSS('border-bottom-width', '1px');
  await expect(dialog.locator('.ol-dialog-footer')).toHaveCSS('border-top-width', '1px');
  await expect(dialog.getByRole('checkbox')).not.toBeChecked();
  await expect(confirm).toBeDisabled();
  await expect(dialog).toContainText('Mivra');
  await expect(dialog.getByText(/uploads your registered API keys/i)).toBeInViewport();
  await expect(dialog).toContainText('XChaCha20-Poly1305');
  await expect(dialog).toContainText('Argon2id');
  await expect
    .poll(() =>
      dialog
        .getByTestId('sync-terms-body')
        .evaluate((body) => body.scrollHeight > body.clientHeight),
    )
    .toBe(true);
  await dialog.getByRole('button', { name: 'Not now' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Sync now', exact: true })).toBeDisabled();
  expect(await attempts(page)).toEqual([]);

  await page.getByRole('button', { name: /disclosure has changed|updated disclosure/i }).click();
  await expect(dialog.getByRole('checkbox')).not.toBeChecked();
  await dialog.getByRole('checkbox').check();
  await confirm.click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([
      {
        command: 'cloud_sync_e2ee_prepare_enable',
        args: {
          consentVersion: 'encrypted-full-snapshot-v2',
          expectedScope: {
            serviceOrigin: 'https://sync.example',
            accountId: '42',
            vaultId: 'old-vault',
          },
        },
      },
    ]);
  await expect(confirm).toBeDisabled();
  await dismissWhilePending(page, dialog);
  await finish(page, true);
  await expect(dialog.getByRole('alert')).toContainText('Could not reach the sync service');
  await confirm.click();
  await expect.poll(async () => (await attempts(page)).length).toBe(2);
  await finish(page);
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Sync now', exact: true })).toBeEnabled();
});

test('a new vault waits for matching NFC passwords and confirmed initialization', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=fresh');
  const enabled = page.getByRole('switch', { name: 'Enable encrypted sync' });
  await enabled.click();
  const disclosure = page.getByRole('dialog');
  await disclosure.getByRole('checkbox').check();
  await disclosure.getByRole('button', { name: 'Agree and continue' }).click();
  await expect.poll(async () => (await attempts(page)).length).toBe(1);
  await finish(page);
  const password = page.getByRole('dialog', { name: 'Set your cloud sync password' });
  const create = password.getByRole('button', { name: 'Confirm and continue' });
  const newPassword = password.getByLabel('Sync password', { exact: true });
  const confirmation = password.getByLabel('Confirm sync password', { exact: true });
  await expect(newPassword).toHaveAttribute('type', 'password');
  await expect(confirmation).toHaveAttribute('type', 'password');
  await newPassword.fill('Test credential 🔒');
  await newPassword.focus();
  await newPassword.evaluate((input: HTMLInputElement) => input.setSelectionRange(1, 7));
  const getSelection = () =>
    newPassword.evaluate((input: HTMLInputElement) => [input.selectionStart, input.selectionEnd]);
  const frame = newPassword.locator('..');
  const measure = () =>
    frame.evaluate((node: HTMLElement) => [node.offsetWidth, node.offsetHeight]);
  const initialBounds = await measure();
  await expect(frame).not.toContainText('Test credential');
  await password.getByRole('button', { name: 'Show password (Sync password)' }).click();
  await expect(newPassword).toHaveAttribute('type', 'text');
  await expect(newPassword).toBeFocused();
  expect(await getSelection()).toEqual([1, 7]);
  expect(await measure()).toEqual(initialBounds);
  await expect(confirmation).toHaveAttribute('type', 'password');
  await password.getByRole('button', { name: 'Hide password (Sync password)' }).click();
  await expect(newPassword).toHaveAttribute('type', 'password');
  expect(await getSelection()).toEqual([1, 7]);
  await expect(newPassword).toHaveValue('Test credential 🔒');
  await newPassword.fill('Aa1short');
  await confirmation.fill('Aa1short');
  await expect(create).toBeDisabled();
  await newPassword.fill('LongFixture1A' + 'x'.repeat(128));
  await confirmation.fill('LongFixture1A' + 'x'.repeat(128));
  await expect(create).toBeDisabled();
  await newPassword.fill('LongFixture1Ae\u0301');
  await confirmation.fill('LongFixture1Ae');
  await expect(confirmation).toHaveAttribute('aria-invalid', 'true');
  await expect(confirmation).toHaveAccessibleDescription(/passwords .* match/i);
  await expect(create).toBeDisabled();
  await confirmation.fill('LongFixture1Aé');
  await create.click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([
      { command: 'cloud_sync_e2ee_prepare_enable' },
      { command: 'cloud_sync_e2ee_create', args: { observedRevision: '7' } },
    ]);
  await expect(enabled).not.toBeChecked();
  await dismissWhilePending(page, password);
  await finish(page, true);
  await expect(password.getByRole('alert')).toContainText('Could not reach the sync service');
  await expect(enabled).not.toBeChecked();
  await expect(newPassword).toHaveValue('');
  await newPassword.fill('LongFixture1Ae\u0301');
  await confirmation.fill('LongFixture1Aé');
  await create.click();
  await expect.poll(async () => (await attempts(page)).length).toBe(3);
  await finish(page);
  await expect(password).toHaveCount(0);
  await expect(enabled).toBeChecked();
});

test('cloud deletion focuses cancel and keeps a failed confirmation open', async ({ page }) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=delete');
  const trigger = page.getByRole('button', { name: 'Delete cloud backup', exact: true });
  await trigger.click();
  const dialog = page.getByRole('dialog', { name: 'Delete the encrypted cloud backup?' });
  const cancel = dialog.getByRole('button', { name: 'Cancel', exact: true });
  const remove = dialog.getByRole('button', { name: 'Delete cloud backup', exact: true });
  await expect(cancel).toBeFocused();
  await expect(dialog).toHaveCSS('width', '400px');
  await expect(dialog.locator('.ol-confirm-dialog-icon')).toHaveCSS('width', '36px');
  await expect(dialog).toHaveCSS('transform', 'none');
  const [cancelBox, removeBox] = await dialog
    .locator('.ol-dialog-footer button')
    .evaluateAll((actions) =>
      actions.map((action) => {
        const { x, width } = action.getBoundingClientRect();
        return { x, width };
      }),
    );
  expect(cancelBox.x).toBeLessThan(removeBox.x);
  expect(Math.abs(cancelBox.width - removeBox.width)).toBeLessThan(1);
  await expect(dialog.getByRole('checkbox')).toHaveCount(0);
  await expect(dialog).toContainText('Local data is kept.');
  await expect(dialog).toContainText('up to 7 days');
  await expect(remove).toHaveClass(/ol-dialog-action--danger/);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeFocused();
  expect(await attempts(page)).toEqual([]);

  await trigger.click();
  await remove.click();
  await expect
    .poll(() => attempts(page))
    .toEqual([
      {
        command: 'cloud_sync_e2ee_delete_remote',
        args: {
          expectedVaultId: 'old-vault',
          observedRevision: '7',
          confirmed: true,
        },
      },
    ]);
  await expect(cancel).toBeDisabled();
  await expect(remove).toBeDisabled();
  await dismissWhilePending(page, dialog);
  await finish(page, true);
  await expect(dialog.getByRole('alert')).toContainText('Could not reach the sync service');
  await expect(remove).toBeEnabled();
  await remove.click();
  await expect.poll(async () => (await attempts(page)).length).toBe(2);
  await finish(page);
  await expect(dialog).toHaveCount(0);
  await expect(trigger).toBeDisabled();
});

test('a declined renewal can inspect and explicitly delete a cold-start backup', async ({
  page,
}) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=renew-cold');
  const renewal = page.getByRole('dialog', { name: /disclosure has changed|updated disclosure/i });
  await renewal.getByRole('button', { name: 'Not now' }).click();
  expect(await attempts(page)).toEqual([]);
  await page.getByRole('button', { name: 'View cloud backup' }).click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([{ command: 'cloud_sync_e2ee_refresh_backup_metadata' }]);
  await page.getByRole('button', { name: 'View cloud backup' }).click();
  await expect.poll(() => attempts(page)).toHaveLength(2);
  await page.getByRole('button', { name: 'Delete cloud backup', exact: true }).click();
  const confirmation = page.getByRole('dialog', { name: 'Delete the encrypted cloud backup?' });
  expect(await attempts(page)).toHaveLength(2);
  await confirmation.getByRole('button', { name: 'Delete cloud backup', exact: true }).click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([
      { command: 'cloud_sync_e2ee_refresh_backup_metadata' },
      { command: 'cloud_sync_e2ee_refresh_backup_metadata' },
      {
        command: 'cloud_sync_e2ee_delete_remote',
        args: { expectedVaultId: 'old-vault', observedRevision: '7', confirmed: true },
      },
    ]);
  await finish(page);
  await expect(confirmation).toHaveCount(0);
});

test('a disabled old backup can be reviewed and deleted after restart', async ({ page }) => {
  await page.goto('/e2e/fixtures/cloud-sync-dialogs.html?mode=renew-cold');
  const renewal = page.getByRole('dialog', { name: /disclosure has changed|updated disclosure/i });
  await renewal.getByRole('button', { name: 'Not now' }).click();
  const toggle = page.getByRole('switch', { name: 'Enable encrypted sync' });
  await toggle.click();
  await page.getByRole('button', { name: 'Turn off sync', exact: true }).click();
  await expect(toggle).not.toBeChecked();
  expect(await attempts(page)).toMatchObject([
    { command: 'cloud_sync_e2ee_set_enabled', args: { enabled: false } },
  ]);
  await page.reload();
  await expect(toggle).not.toBeChecked();
  expect(await attempts(page)).toEqual([]);
  await page.getByRole('button', { name: 'View cloud backup' }).click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([{ command: 'cloud_sync_e2ee_refresh_backup_metadata' }]);
  await expect(toggle).not.toBeChecked();
  await page.getByRole('button', { name: 'Delete cloud backup', exact: true }).click();
  const confirmation = page.getByRole('dialog', { name: 'Delete the encrypted cloud backup?' });
  expect(await attempts(page)).toHaveLength(1);
  await confirmation.getByRole('button', { name: 'Delete cloud backup', exact: true }).click();
  await expect
    .poll(() => attempts(page))
    .toMatchObject([
      { command: 'cloud_sync_e2ee_refresh_backup_metadata' },
      {
        command: 'cloud_sync_e2ee_delete_remote',
        args: { expectedVaultId: 'old-vault', observedRevision: '7', confirmed: true },
      },
    ]);
  await finish(page);
  await expect(confirmation).toHaveCount(0);
  await expect(toggle).not.toBeChecked();
});
