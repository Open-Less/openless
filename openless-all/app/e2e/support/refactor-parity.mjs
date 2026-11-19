import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

// Keep this function self-contained so the same read-only probe runs in a packaged WebView.
export async function readRefactorParity(selectors = {}) {
  const html = document.documentElement;
  const rect = ({ left, top, width, height }) => ({ x: left, y: top, width, height });
  const name = (node) =>
    node ? [node.localName, node.id || undefined, node.getAttribute('class') || undefined] : null;
  function inspect(node) {
    const box = node.getBoundingClientRect();
    const style = getComputedStyle(node);
    const points = [
      [box.left + box.width / 2, box.top + box.height / 2],
      [box.left + 2, box.top + 2],
      [box.right - 2, box.bottom - 2],
    ];
    const clippingAncestors = [];
    for (let parent = node.parentElement; parent; parent = parent.parentElement) {
      const { overflowX, overflowY, clipPath, contain } = getComputedStyle(parent);
      if (
        [overflowX, overflowY].some((value) => value !== 'visible') ||
        clipPath !== 'none' ||
        /paint|content|strict/.test(contain)
      ) {
        clippingAncestors.push({
          node: name(parent),
          overflowX,
          overflowY,
          clipPath,
          contain,
          rect: rect(parent.getBoundingClientRect()),
        });
      }
    }
    return {
      rect: rect(box),
      intrinsic: { width: node.offsetWidth, height: node.offsetHeight },
      scroll: { width: node.scrollWidth, height: node.scrollHeight },
      paint: {
        clipPath: style.clipPath,
        cornerShape: style.getPropertyValue('corner-shape'),
        borderRadius: style.borderRadius,
        color: style.color,
        background: style.backgroundColor,
        visibility: style.visibility,
        pointerEvents: style.pointerEvents,
      },
      hit: points.map(([x, y]) => {
        const top = document.elementFromPoint(x, y);
        return { x, y, selfOrChild: top !== null && node.contains(top), top: name(top) };
      }),
      clippingAncestors,
    };
  }

  const bridge = window.__TAURI_INTERNALS__;
  const label = bridge?.metadata?.currentWindow?.label;
  const native =
    label && bridge.invoke
      ? Object.fromEntries(
          await Promise.all(
            ['inner_size', 'inner_position', 'outer_size', 'scale_factor'].map(async (key) => {
              try {
                return [key, await bridge.invoke(`plugin:window|${key}`, { label })];
              } catch {
                return [key, null];
              }
            }),
          ),
        )
      : null;
  const query = new URLSearchParams(location.search);
  const visual = window.visualViewport;
  let selectedScale = null;
  try {
    selectedScale = localStorage.getItem('ol-font-scale');
  } catch {
    // Some WebViews cannot read storage for an opaque origin.
  }
  return {
    engine: navigator.userAgent,
    host: bridge ? 'tauri' : window.OpenLessHarmonyApi ? 'arkweb' : 'browser',
    route: query.get('window') ?? 'main',
    windowLabel: label ?? null,
    native,
    environment: {
      platform: html.dataset.olPlatform,
      locale: html.lang,
      theme: getComputedStyle(html).colorScheme,
      layout: html.dataset.olBrowserFrame ?? 'unframed',
      webdriver: navigator.webdriver,
      selectedScale,
      declaredScale: getComputedStyle(html).getPropertyValue('--ol-interface-scale').trim(),
      rootZoom: getComputedStyle(html).zoom,
    },
    viewport: {
      width: innerWidth,
      height: innerHeight,
      dpr: devicePixelRatio,
      visual: visual && { width: visual.width, height: visual.height, scale: visual.scale },
      available: { width: screen.availWidth, height: screen.availHeight },
    },
    focused: name(document.activeElement),
    targets: Object.fromEntries(
      Object.entries({
        root: '#root',
        shell: '.ol-app-shell-bg',
        dashboard: '.ol-overview-dashboard',
        selected:
          '.ol-overview-dashboard [aria-pressed="true"], .ol-overview-segment [aria-pressed="true"]',
        heatmap: '.ol-gha',
        cell: '.ol-gha-cell',
        year: '.ol-overview-year',
        overlay: '.ol-dialog-overlay, .ol-modal-backdrop',
        dialog: '.ol-confirm-dialog, [role="dialog"]',
        ...selectors,
      }).map(([key, selector]) => {
        const node = document.querySelector(selector);
        return [key, node ? inspect(node) : null];
      }),
    ),
  };
}

async function run() {
  const { values } = parseArgs({
    options: {
      url: { type: 'string' },
      sha: { type: 'string' },
      inspector: { type: 'boolean' },
      selector: { type: 'string', multiple: true },
      ordinary: { type: 'boolean' },
      history: { type: 'boolean' },
      size: { type: 'string', default: '1300x835' },
      dpr: { type: 'string', default: '1' },
      locale: { type: 'string', default: 'zh-CN' },
      theme: { type: 'string', default: 'dark' },
      scale: { type: 'string', multiple: true },
      screenshots: { type: 'string' },
      timezone: { type: 'string', default: 'America/Denver' },
      date: { type: 'string', default: '2026-11-17T19:00:00Z' },
      ready: { type: 'string', default: '.ol-app-shell-bg' },
    },
  });
  const selectors = Object.fromEntries(
    (values.selector ?? []).map((option) => {
      const colon = option.indexOf(':');
      if (colon < 1 || colon === option.length - 1) throw new Error('--selector requires name:CSS');
      return [option.slice(0, colon), option.slice(colon + 1)];
    }),
  );
  if (values.inspector) {
    console.log(
      `(${readRefactorParity.toString()})(${JSON.stringify(selectors)}).then(JSON.stringify).then(console.log)`,
    );
    return;
  }

  const [width, height] = values.size.split('x').map(Number);
  const deviceScaleFactor = Number(values.dpr);
  const scaleIds = { 90: 'small', 100: 'medium', 110: 'large' };
  const url = new URL(values.url ?? 'http://127.0.0.1:1420/?os=mac');
  const scales = values.scale ?? Object.keys(scaleIds);
  if (
    !/^[a-f\d]{7,40}$/.test(values.sha ?? '') ||
    !Number.isInteger(width) ||
    !Number.isInteger(height) ||
    width < 1 ||
    height < 1 ||
    !Number.isFinite(deviceScaleFactor) ||
    deviceScaleFactor <= 0 ||
    !['light', 'dark'].includes(values.theme) ||
    scales.some((scale) => !scaleIds[scale])
  ) {
    throw new Error(
      'Supply --sha and valid --size, --dpr, --theme, and --scale 90|100|110 values.',
    );
  }
  if (values.history && values.locale !== 'zh-CN')
    throw new Error('--history uses the zh-CN button labels.');
  const { chromium } = await import('@playwright/test');
  const browser = await chromium.launch();
  if (values.screenshots) await mkdir(values.screenshots, { recursive: true });
  try {
    for (const scale of scales) {
      const page = await browser.newPage({
        viewport: { width, height },
        deviceScaleFactor,
        locale: values.locale,
        colorScheme: values.theme,
        timezoneId: values.timezone,
      });
      await page.clock.setFixedTime(new Date(values.date));
      await page.addInitScript(
        ({ locale, theme, scaleId, ordinary }) => {
          localStorage.setItem('ol.locale', locale);
          localStorage.setItem('ol.theme', theme);
          localStorage.setItem('ol-font-scale', scaleId);
          if (ordinary) Object.defineProperty(navigator, 'webdriver', { get: () => false });
        },
        {
          locale: values.locale,
          theme: values.theme,
          scaleId: scaleIds[scale],
          ordinary: values.ordinary,
        },
      );
      await page.goto(url.toString());
      await page.locator(values.ready).first().waitFor({ state: 'visible' });
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      async function capture(stage, systemDialog = null) {
        await page.evaluate(async () => {
          // Continuous indicators never settle and do not affect these measurements.
          const transitions = document
            .getAnimations()
            .filter(
              (animation) =>
                animation.playState === 'running' &&
                Number.isFinite(animation.effect?.getComputedTiming().endTime),
            );
          await Promise.allSettled(transitions.map((animation) => animation.finished));
        });
        if (values.screenshots)
          await page.screenshot({
            path: resolve(values.screenshots, `${stage}-${scale}.png`),
            animations: 'disabled',
          });
        console.log(
          JSON.stringify({
            sha: values.sha,
            fixture: 'repository mock (fixed JS date)',
            date: values.date,
            url: url.toString(),
            stage,
            requestedScale: Number(scale),
            systemDialog,
            ...(await page.evaluate(readRefactorParity, selectors)),
          }),
        );
      }
      await capture('initial');
      if (values.history) {
        await page
          .locator('.ol-sidebar-surface')
          .getByRole('button', { name: '历史', exact: true })
          .click();
        let systemDialog = null;
        page.once('dialog', async (dialog) => {
          systemDialog = dialog.type();
          await dialog.dismiss();
        });
        await page.getByRole('button', { name: '清空', exact: true }).click();
        if (!systemDialog) await page.getByRole('button', { name: '取消', exact: true }).waitFor();
        await capture('clear-confirm', systemDialog);
        if (!systemDialog) await page.getByRole('button', { name: '取消', exact: true }).click();
      }
      await page.close();
    }
  } finally {
    await browser.close();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await run();
}
