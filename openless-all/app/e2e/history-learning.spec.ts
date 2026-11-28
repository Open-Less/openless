import { expect, test } from '@playwright/test';
import { fixtureOrigin } from './support/server';

declare global {
  interface Window {
    historyLearningFixture: {
      calls: unknown[][];
      holdBegin: boolean;
      releaseBegin: () => void;
      expireOnAccept: boolean;
    };
  }
}

test('history learning uses a frozen source and releases it on close', async ({ page }) => {
  await page.setViewportSize({ width: 880, height: 620 });
  await page.route('**/src/lib/ipc/mock-data.ts*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      body: `${await response.text()}
mockSettings.vocabularyLearningEnabled = true;
mockSettings.expressionLearningEnabled = true;`,
    });
  });
  await page.route('**/src/lib/ipc/historyLearning.ts*', (route) => route.fulfill({
    contentType: 'text/javascript',
    body: `
const fixture = window.historyLearningFixture = { calls: [], holdBegin: false, expireOnAccept: false };
export async function beginHistoryLearning(historyId) {
  fixture.calls.push(['begin', historyId]);
  if (fixture.holdBegin) await new Promise((resolve) => { fixture.releaseBegin = resolve; });
  return { token: 'history-token', historyId, originalText: 'Use codx here', expiresAtMs: Date.now() + 300000 };
}
export async function submitHistoryLearning(token, correctedText, vocabulary, expression) {
  fixture.calls.push(['review', token, correctedText, vocabulary, expression]);
  return { candidates: [{ id: 'candidate', pattern: 'codx', replacement: 'codex' }], expressionRecorded: false };
}
export async function acceptHistoryLearning(token, candidateId) {
  fixture.calls.push(['accept', token, candidateId]);
  if (fixture.expireOnAccept) throw 'learningSourceExpired';
  return true;
}
export async function cancelHistoryLearning(token) { fixture.calls.push(['cancel', token]); }
`,
  }));
  await page.goto(`${fixtureOrigin}/?os=mac`);
  await page.getByRole('button', { name: '历史', exact: true }).click();
  await page.locator('[data-history-record="mock-0"]').click();
  const actions = page.getByRole('button', { name: '录音操作', exact: true });
  const dialog = page.getByRole('dialog', { name: '修正并学习', exact: true });
  const open = async () => {
    await actions.click();
    await page.getByRole('menuitem', { name: '修正并学习', exact: true }).click();
  };
  const corrected = dialog.getByRole('textbox', { name: '修正后的文字' });
  const expression = dialog.getByRole('checkbox', { name: '学习表达习惯' });
  expect(await page.evaluate(() => window.historyLearningFixture.calls)).toEqual([]);

  await open();
  await expect(corrected).toBeFocused();
  await expect(corrected).toHaveValue('Use codx here');
  await expect(expression).not.toBeChecked();
  await corrected.fill('Use codex here');
  await dialog.getByRole('button', { name: '检查修正' }).click();
  await expect(dialog.getByText('历史修正', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.historyLearningFixture.calls)).toEqual([
    ['begin', 'mock-0'],
    ['review', 'history-token', 'Use codex here', true, false],
  ]);
  await dialog.getByRole('button', { name: '加入词库', exact: true }).click();
  await expect(dialog.getByText('已在词库中。')).toBeVisible();
  await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(actions).toBeFocused();
  await expect.poll(() => page.evaluate(() => window.historyLearningFixture.calls.slice(-2))).toEqual([
    ['accept', 'history-token', 'candidate'], ['cancel', 'history-token'],
  ]);

  await page.evaluate(() => { window.historyLearningFixture.expireOnAccept = true; });
  await open();
  await expression.check();
  await corrected.fill('Use codex here!');
  await dialog.getByRole('button', { name: '检查修正' }).click();
  await dialog.getByRole('button', { name: '加入词库', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText('历史已变更或修正已过期，请重新打开。');
  await expect(dialog.getByRole('button', { name: '加入词库', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => window.historyLearningFixture.calls.slice(-3))).toEqual([
    ['review', 'history-token', 'Use codex here!', true, true],
    ['accept', 'history-token', 'candidate'],
    ['cancel', 'history-token'],
  ]);
  await page.keyboard.press('Escape');
  await expect(dialog).toHaveCount(0);

  await page.evaluate(() => { window.historyLearningFixture.holdBegin = true; });
  await open();
  await expect(dialog.getByRole('status')).toBeVisible();
  await dialog.getByRole('button', { name: '取消' }).click();
  await expect(dialog).toHaveCount(0);
  await page.evaluate(() => window.historyLearningFixture.releaseBegin());
  await expect.poll(() => page.evaluate(() => window.historyLearningFixture.calls.slice(-2))).toEqual([
    ['begin', 'mock-0'], ['cancel', 'history-token'],
  ]);
});
