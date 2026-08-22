import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const APP_URL = process.env.OPENLESS_H5_URL || 'http://127.0.0.1:1420/';
const CHROME_PATH = process.env.CHROME_PATH || 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const OUTPUT_DIR = process.env.OPENLESS_LAYOUT_ARTIFACT_DIR || join(tmpdir(), 'openless-layout-h5');
const VIEWPORTS = [
  { width: 360, height: 640 },
  { width: 320, height: 568 },
];
const ZOOMS = [1.1, 2];

mkdirSync(OUTPUT_DIR, { recursive: true });

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

class CdpClient {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();
    socket.addEventListener('message', event => {
      const message = JSON.parse(String(event.data));
      if (!message.id) return;
      const waiter = this.pending.get(message.id);
      if (!waiter) return;
      this.pending.delete(message.id);
      if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
      else waiter.resolve(message.result);
    });
  }

  static async connect(url) {
    const socket = new WebSocket(url);
    await new Promise((resolve, reject) => {
      socket.addEventListener('open', resolve, { once: true });
      socket.addEventListener('error', reject, { once: true });
    });
    return new CdpClient(socket);
  }

  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    this.socket.close();
  }
}

async function waitForPageTarget(port) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch('http://127.0.0.1:' + port + '/json/list');
      const targets = await response.json();
      const target = targets.find(item => item.type === 'page' && item.url.startsWith(APP_URL));
      if (target?.webSocketDebuggerUrl) return target;
    } catch {
      // Chrome is still starting.
    }
    await sleep(100);
  }
  throw new Error('Chrome DevTools target did not become ready');
}

async function launchChrome() {
  const port = 9300 + Math.floor(Math.random() * 400);
  const profile = join(tmpdir(), 'openless-layout-cdp-' + Date.now());
  const child = spawn(CHROME_PATH, [
    '--headless=new',
    '--disable-gpu',
    '--no-first-run',
    '--no-default-browser-check',
    '--remote-debugging-port=' + port,
    '--user-data-dir=' + profile,
    APP_URL,
  ], { stdio: 'ignore' });
  const target = await waitForPageTarget(port);
  return { child, client: await CdpClient.connect(target.webSocketDebuggerUrl) };
}

async function evaluate(client, expression) {
  const response = await client.send('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (response.exceptionDetails) {
    throw new Error(
      response.exceptionDetails.exception?.description || response.exceptionDetails.text || 'Runtime.evaluate failed',
    );
  }
  return response.result.value;
}

async function evaluateFn(client, fn, ...args) {
  const expression = '(' + fn.toString() + ')(' + args.map(value => JSON.stringify(value)).join(',') + ')';
  return evaluate(client, expression);
}

async function waitForFn(client, fn, args = [], timeoutMs = 10000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await evaluateFn(client, fn, ...args)) return;
    await sleep(100);
  }
  const diagnostics = await evaluateFn(client, function collectDiagnostics() {
    const visible = element => element.getBoundingClientRect().width > 0;
    return {
      buttons: Array.from(document.querySelectorAll('button')).filter(visible)
        .map(button => button.innerText.trim()).filter(Boolean).slice(0, 40),
      leaves: Array.from(document.querySelectorAll('div,span')).filter(element => (
        visible(element) && element.children.length === 0 && element.textContent.trim()
      )).map(element => element.textContent.trim()).slice(0, 80),
      layoutTexts: document.body.innerText.split('\n')
        .map(text => text.trim()).filter(text => /布局|排版|易读/.test(text)),
    };
  });
  throw new Error(
    'Timed out waiting for browser state: ' + fn.name + ' diagnostics=' + JSON.stringify(diagnostics),
  );
}

function hasExactButton(text) {
  return Array.from(document.querySelectorAll('button')).some(button => (
    button.getBoundingClientRect().width > 0 && button.innerText.trim() === text
  ));
}

function clickExactButton(text) {
  const button = Array.from(document.querySelectorAll('button')).find(candidate => (
    candidate.getBoundingClientRect().width > 0 && candidate.innerText.trim() === text
  ));
  if (!button) {
    return {
      ok: false,
      buttons: Array.from(document.querySelectorAll('button'))
        .filter(candidate => candidate.getBoundingClientRect().width > 0)
        .map(candidate => candidate.innerText.trim())
        .filter(Boolean),
    };
  }
  button.click();
  return { ok: true };
}

async function clickButton(client, text) {
  const result = await evaluateFn(client, clickExactButton, text);
  assert.equal(result.ok, true, '找不到按钮 ' + text + '，当前按钮：' + JSON.stringify(result.buttons));
  await sleep(180);
}

function clickCompositeRow(text) {
  const button = Array.from(document.querySelectorAll('button.ol-nav-btn')).find(candidate => (
    candidate.children.length === 3 &&
    candidate.getBoundingClientRect().width > 0 &&
    candidate.innerText.trim() === text
  ));
  if (!button) return false;
  button.click();
  return true;
}

function hasSettingLabel(label) {
  return document.body.innerText.includes(label);
}

async function openSettings(client) {
  await clickButton(client, '更多');
  await waitForFn(client, hasExactButton, ['设置']);
  await clickButton(client, '设置');
  await waitForFn(client, hasExactButton, ['通用']);
  await clickButton(client, '通用');
  await waitForFn(client, hasSettingLabel, ['易读布局（防溢出换行）']);
}

function closeCurrentOverlay() {
  const buttons = Array.from(document.querySelectorAll('button[aria-label="关闭"]'))
    .filter(button => button.getBoundingClientRect().width > 0);
  if (!buttons.length) return false;
  buttons[0].click();
  return true;
}

async function closeOverlay(client) {
  assert.equal(await evaluateFn(client, closeCurrentOverlay), true, '找不到可见的关闭按钮');
  await sleep(220);
}

function clickSettingToggle(label) {
  const labelNode = Array.from(document.querySelectorAll('div,span'))
    .find(node => node.textContent.trim() === label);
  let row = labelNode;
  while (row && !row.querySelector('button')) row = row.parentElement;
  const button = row?.querySelector('button');
  if (!button) return false;
  button.click();
  return true;
}

function rootPreferenceState() {
  return {
    readable: document.documentElement.dataset.olStackedLayout === 'true',
    conservative: document.documentElement.dataset.olConservativeLayout === 'true',
  };
}

async function setPreference(client, label, key, desired) {
  const state = await evaluateFn(client, rootPreferenceState);
  if (state[key] !== desired) {
    assert.equal(await evaluateFn(client, clickSettingToggle, label), true, '找不到布局开关：' + label);
  }
  await waitForFn(client, function waitPreference(prefKey, expected) {
    const root = document.documentElement.dataset;
    const actual = prefKey === 'readable'
      ? root.olStackedLayout === 'true'
      : root.olConservativeLayout === 'true';
    return actual === expected;
  }, [key, desired]);
  await sleep(180);
}

async function setPreferences(client, readable, conservative) {
  await clickButton(client, '通用');
  await waitForFn(client, hasSettingLabel, ['易读布局（防溢出换行）']);
  await setPreference(client, '易读布局（防溢出换行）', 'readable', readable);
  await setPreference(client, '保守排版', 'conservative', conservative);
}

function inspectGeneralLayout(zoom) {
  const inspectToggle = label => {
    const labelNode = Array.from(document.querySelectorAll('div,span'))
      .find(node => node.textContent.trim() === label);
    let row = labelNode;
    while (row && !row.querySelector('button')) row = row.parentElement;
    const button = row?.querySelector('button');
    const control = button?.parentElement;
    if (!row || !button || !control) return null;
    const buttonRect = button.getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    return {
      normalizedWidth: Number((buttonRect.width / zoom).toFixed(2)),
      buttonLeft: Number(buttonRect.left.toFixed(2)),
      rowLeft: Number(rowRect.left.toFixed(2)),
      rowRight: Number(rowRect.right.toFixed(2)),
      justifyContent: getComputedStyle(control).justifyContent,
    };
  };

  const settingsRoot = document.querySelector('.ol-settings-surface') || document.body;
  const isScrollContained = element => {
    for (let parent = element.parentElement; parent && parent !== settingsRoot; parent = parent.parentElement) {
      const overflowX = getComputedStyle(parent).overflowX;
      if (overflowX === 'auto' || overflowX === 'scroll') return true;
    }
    return false;
  };
  const outliers = Array.from(settingsRoot.querySelectorAll('button,input,select,textarea,[role="button"],.ol-inline-composite'))
    .filter(element => {
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && (rect.left < -1 || rect.right > innerWidth + 1);
    })
    .filter(element => !isScrollContained(element))
    .slice(0, 12)
    .map(element => ({
      tag: element.tagName,
      text: element.textContent.trim().slice(0, 30),
      rect: Array.from([
        element.getBoundingClientRect().left,
        element.getBoundingClientRect().right,
        innerWidth,
      ]).map(value => Number(value.toFixed(1))),
    }));

  return {
    root: {
      readable: document.documentElement.dataset.olStackedLayout === 'true',
      conservative: document.documentElement.dataset.olConservativeLayout === 'true',
    },
    readableToggle: inspectToggle('易读布局（防溢出换行）'),
    conservativeToggle: inspectToggle('保守排版'),
    outliers,
  };
}

function inspectServiceActions() {
  const edit = Array.from(document.querySelectorAll('button')).find(button => (
    button.getBoundingClientRect().width > 0 &&
    (button.getAttribute('aria-label') || '').startsWith('编辑')
  ));
  const group = edit?.parentElement;
  if (!group) return null;
  const style = getComputedStyle(group);
  const childTops = Array.from(group.children)
    .filter(child => child.getBoundingClientRect().height > 0)
    .map(child => Math.round(child.getBoundingClientRect().top));
  return {
    className: group.className,
    flexDirection: style.flexDirection,
    alignItems: style.alignItems,
    childCount: childTops.length,
    uniqueRows: new Set(childTops).size,
  };
}

function inspectPermissionActions() {
  const surface = document.querySelector('.ol-settings-surface');
  const labelNode = Array.from(surface?.querySelectorAll('div,span') || [])
    .find(node => node.textContent.trim() === '麦克风');
  let row = labelNode;
  while (row && getComputedStyle(row).display !== 'grid') row = row.parentElement;
  const group = row?.children[1]?.firstElementChild;
  if (!group) return null;
  const style = getComputedStyle(group);
  return {
    flexDirection: style.flexDirection,
    alignItems: style.alignItems,
    justifyContent: style.justifyContent,
  };
}

function inspectAboutComposite() {
  const row = document.querySelector('.ol-inline-composite');
  if (!row) return null;
  const rowRect = row.getBoundingClientRect();
  const children = Array.from(row.children).map(child => {
    const rect = child.getBoundingClientRect();
    return {
      left: Number(rect.left.toFixed(1)),
      right: Number(rect.right.toFixed(1)),
      top: Number(rect.top.toFixed(1)),
      bottom: Number(rect.bottom.toFixed(1)),
    };
  });
  const style = getComputedStyle(row);
  return {
    flexDirection: style.flexDirection,
    row: [rowRect.left, rowRect.right, rowRect.top, rowRect.bottom].map(value => Number(value.toFixed(1))),
    flexWrap: style.flexWrap,
    childCount: children.length,
    withinRow: children.every(rect => rect.left >= rowRect.left - 1 && rect.right <= rowRect.right + 1),
    children,
  };
}

function inspectCompositeSheet(expectedRows) {
  const rows = Array.from(document.querySelectorAll('button.ol-nav-btn')).filter(button => (
    button.children.length === 3 &&
    button.getBoundingClientRect().width > 0 &&
    getComputedStyle(button).flexDirection === 'row'
  ));
  return {
    count: rows.length,
    expectedRows,
    valid: rows.every(row => {
      const rect = row.getBoundingClientRect();
      const children = Array.from(row.children).map(child => child.getBoundingClientRect());
      return rect.left >= -1 &&
        rect.right <= innerWidth + 1 &&
        children.every(child => child.top < rect.bottom && child.bottom > rect.top);
    }),
    rows: rows.map(row => {
      const rect = row.getBoundingClientRect();
      const parentRect = row.parentElement.getBoundingClientRect();
      const style = getComputedStyle(row);
      return {
        text: row.innerText.trim(),
        rect: [rect.left, rect.right, rect.top, rect.bottom, innerWidth].map(value => Number(value.toFixed(1))),
        parentRect: [parentRect.left, parentRect.right, parentRect.width].map(value => Number(value.toFixed(1))),
        computed: { boxSizing: style.boxSizing, width: style.width, padding: style.padding },
        children: Array.from(row.children).map(child => {
          const childRect = child.getBoundingClientRect();
          return [childRect.left, childRect.right, childRect.top, childRect.bottom].map(value => Number(value.toFixed(1)));
        }),
      };
    }),
  };
}

function inspectPageOutliers() {
  const root = document.querySelector('main');
  if (!root) return [{ tag: 'MISSING_MAIN' }];
  const isScrollContained = element => {
    for (let parent = element.parentElement; parent && parent !== root; parent = parent.parentElement) {
      const overflowX = getComputedStyle(parent).overflowX;
      if (overflowX === 'auto' || overflowX === 'scroll') return true;
    }
    return false;
  };
  return Array.from(root.querySelectorAll('h1,p,button,input,select,textarea'))
    .filter(element => {
      const rect = element.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && (rect.left < -1 || rect.right > innerWidth + 1);
    })
    .filter(element => !isScrollContained(element))
    .slice(0, 12)
    .map(element => {
      const rect = element.getBoundingClientRect();
      return {
        tag: element.tagName,
        text: element.textContent.trim().slice(0, 30),
        rect: [
          Number(rect.left.toFixed(1)),
          Number(rect.right.toFixed(1)),
          innerWidth,
        ],
      };
    });
}
function pageGridFingerprint() {
  const grid = document.querySelector('.ol-grid-auto-cards');
  if (!grid) return null;
  const rect = grid.getBoundingClientRect();
  return {
    columns: getComputedStyle(grid).gridTemplateColumns,
    width: Number(rect.width.toFixed(1)),
  };
}

async function openStylePage(client, label) {
  await clickButton(client, '风格');
  await waitForFn(client, function hasComposite(labelText) {
    return Array.from(document.querySelectorAll('button.ol-nav-btn')).some(button => (
      button.children.length === 3 &&
      button.getBoundingClientRect().width > 0 &&
      button.innerText.trim() === labelText
    ));
  }, [label]);
  assert.equal(await evaluateFn(client, clickCompositeRow, label), true, '找不到风格抽屉入口：' + label);
  await waitForFn(client, function hasGrid() {
    return Boolean(document.querySelector('.ol-grid-auto-cards'));
  });
  await sleep(300);
}

async function assertCompositeSheets(client) {
  await clickButton(client, '更多');
  await sleep(120);
  const more = await evaluateFn(client, inspectCompositeSheet, 4);
  assert.equal(more.count, 4, '更多抽屉应有四个三元素复合行');
  assert.equal(more.valid, true, '更多抽屉复合行溢出或未保持同行：' + JSON.stringify(more));
  await closeOverlay(client);

  await clickButton(client, '风格');
  await sleep(120);
  const style = await evaluateFn(client, inspectCompositeSheet, 2);
  assert.equal(style.count, 2, '风格抽屉应有两个三元素复合行');
  assert.equal(style.valid, true, '风格抽屉复合行溢出或未保持同行：' + JSON.stringify(style));
  await closeOverlay(client);
}

async function captureScreenshot(client, filename) {
  const result = await client.send('Page.captureScreenshot', {
    format: 'png',
    captureBeyondViewport: false,
  });
  const path = join(OUTPUT_DIR, filename);
  writeFileSync(path, Buffer.from(result.data, 'base64'));
  return path;
}

async function runCase(client, viewport, zoom) {
  await client.send('Emulation.setDeviceMetricsOverride', {
    width: viewport.width,
    height: viewport.height,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await client.send('Page.navigate', { url: APP_URL });
  await waitForFn(client, hasExactButton, ['更多'], 15000);
  await evaluateFn(client, function applyTestZoom(value) {
    document.documentElement.style.zoom = String(value);
  }, zoom);
  await sleep(250);

  await openSettings(client);
  await setPreferences(client, false, false);
  const baselineGeneral = await evaluateFn(client, inspectGeneralLayout, zoom);

  await closeOverlay(client);
  await openStylePage(client, '润色模式');
  const baselineStyle = await evaluateFn(client, pageGridFingerprint);
  await openStylePage(client, '风格市场');
  const baselineMarketplace = await evaluateFn(client, pageGridFingerprint);

  await openSettings(client);
  const combinations = [
    [false, false],
    [true, false],
    [false, true],
    [true, true],
  ];

  const results = [];
  for (const combination of combinations) {
    const readable = combination[0];
    const conservative = combination[1];
    await setPreferences(client, readable, conservative);
    const general = await evaluateFn(client, inspectGeneralLayout, zoom);
    assert.deepEqual(general.root, { readable, conservative });
    for (const toggle of [general.readableToggle, general.conservativeToggle]) {
      assert.ok(toggle, '布局开关未渲染');
      assert.ok(Math.abs(toggle.normalizedWidth - 36) <= 0.75, '开关宽度不再是 36px：' + JSON.stringify(toggle));
      assert.equal(toggle.justifyContent, 'flex-start', '小尺寸布局开关应靠左');
      assert.ok(toggle.buttonLeft >= -1 && toggle.buttonLeft <= toggle.rowRight, '布局开关超出设置行');
    }
    assert.deepEqual(general.outliers, [], '设置页存在未被滚动容器承接的横向溢出');

    if (!readable && conservative) {
      await clickButton(client, '服务');
      await waitForFn(client, function hasEditAction() {
        return Array.from(document.querySelectorAll('button')).some(button => (
          button.getBoundingClientRect().width > 0 &&
          (button.getAttribute('aria-label') || '').startsWith('编辑')
        ));
      });
      const service = await evaluateFn(client, inspectServiceActions);
      assert.ok(service, '服务卡片动作组未渲染');
      assert.equal(service.flexDirection, 'column');
      assert.equal(service.alignItems, 'flex-start');
      assert.ok(service.childCount === 3 && service.uniqueRows === 3, '服务卡片应有验证、开关、编辑三个纵排动作');

      await clickButton(client, '隐私');
      await waitForFn(client, hasSettingLabel, ['麦克风']);
      const privacy = await evaluateFn(client, inspectPermissionActions);
      assert.deepEqual(privacy, {
        flexDirection: 'column',
        alignItems: 'flex-start',
        justifyContent: 'flex-start',
      });

      await clickButton(client, '关于');
      await waitForFn(client, function hasAboutComposite() {
        return Boolean(document.querySelector('.ol-inline-composite'));
      });
      const about = await evaluateFn(client, inspectAboutComposite);
      assert.ok(about, '关于页复合行未渲染');
      assert.equal(about.flexDirection, 'row');
      assert.equal(about.flexWrap, 'nowrap');
      assert.equal(about.childCount, 3);
      assert.equal(about.withinRow, true, '关于页三元素未保持在容器内：' + JSON.stringify(about));
      await closeOverlay(client);
      await openStylePage(client, '润色模式');
      const styleOutliers = await evaluateFn(client, inspectPageOutliers);
      assert.deepEqual(styleOutliers, [], '保守排版下风格页存在横向溢出：' + JSON.stringify(styleOutliers));
      await openStylePage(client, '风格市场');
      const marketplaceOutliers = await evaluateFn(client, inspectPageOutliers);
      assert.deepEqual(marketplaceOutliers, [], '保守排版下风格市场存在横向溢出：' + JSON.stringify(marketplaceOutliers));
      await openSettings(client);
    }

    results.push({ readable, conservative, outliers: general.outliers.length });
  }

  await setPreferences(client, false, false);
  const restoredGeneral = await evaluateFn(client, inspectGeneralLayout, zoom);
  assert.deepEqual(restoredGeneral.root, { readable: false, conservative: false });
  assert.deepEqual(restoredGeneral.readableToggle, baselineGeneral.readableToggle);
  assert.deepEqual(restoredGeneral.conservativeToggle, baselineGeneral.conservativeToggle);

  await closeOverlay(client);
  await openStylePage(client, '润色模式');
  const restoredStyle = await evaluateFn(client, pageGridFingerprint);
  assert.deepEqual(restoredStyle, baselineStyle, '关闭两个偏好后风格页未恢复基线布局');
  await openStylePage(client, '风格市场');
  const restoredMarketplace = await evaluateFn(client, pageGridFingerprint);
  assert.deepEqual(restoredMarketplace, baselineMarketplace, '关闭两个偏好后风格市场未恢复基线布局');

  await assertCompositeSheets(client);
  await openSettings(client);
  await setPreferences(client, false, true);
  await closeOverlay(client);
  await openStylePage(client, '风格市场');
  const screenshot = await captureScreenshot(
    client,
    'layout-' + viewport.width + 'x' + viewport.height + '-zoom-' + String(zoom).replace('.', '_') + '.png',
  );

  return {
    viewport,
    zoom,
    combinations: results,
    baselineStyle,
    baselineMarketplace,
    screenshot,
  };
}

const { child, client } = await launchChrome();
const reports = [];

try {
  await client.send('Page.enable');
  await client.send('Runtime.enable');
  for (const viewport of VIEWPORTS) {
    for (const zoom of ZOOMS) {
      const report = await runCase(client, viewport, zoom);
      reports.push(report);
      console.log('[layout-h5] passed ' + viewport.width + 'x' + viewport.height + ' zoom=' + zoom);
    }
  }
  const reportPath = join(OUTPUT_DIR, 'layout-report.json');
  writeFileSync(reportPath, JSON.stringify(reports, null, 2));
  console.log('[layout-h5] report ' + reportPath);
} finally {
  try {
    await client.send('Browser.close');
  } catch {
    child.kill();
  }
  client.close();
}
