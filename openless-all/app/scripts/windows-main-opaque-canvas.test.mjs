import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const mainTsx = await readFile(new URL("../src/main.tsx", import.meta.url), "utf8");
const globalCss = await readFile(new URL("../src/styles/global.css", import.meta.url), "utf8");

assert.match(
  mainTsx,
  /const openlessWindowKind = isCapsule \? "capsule" : isQa \? "qa" : "main";[\s\S]*const openlessPlatform = detectOS\(\);[\s\S]*document\.documentElement\.dataset\.openlessWindow = openlessWindowKind;[\s\S]*document\.documentElement\.dataset\.openlessPlatform = openlessPlatform;[\s\S]*document\.body\.dataset\.openlessWindow = openlessWindowKind;[\s\S]*document\.body\.dataset\.openlessPlatform = openlessPlatform;/,
  "frontend should mark each webview kind and platform before rendering",
);

assert.match(
  globalCss,
  /html, body, #root \{[\s\S]*background: transparent;[\s\S]*\}/,
  "capsule and QA windows should keep the shared transparent WebView baseline",
);

assert.match(
  globalCss,
  /html\[data-openless-window="main"\]\[data-openless-platform="win"\],[\s\S]*html\[data-openless-window="main"\]\[data-openless-platform="win"\] body,[\s\S]*html\[data-openless-window="main"\]\[data-openless-platform="win"\] #root \{[\s\S]*background: #f5f5f7;[\s\S]*\}/,
  "Windows main window should have an internal opaque canvas to prevent transparent WebView bleed-through",
);
