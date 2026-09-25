import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { createServer } from 'vite';

const out =
  process.env.OPENLESS_MOTION_ARTIFACT_DIR || join(tmpdir(), 'openless-bailian-protocol-h5');
mkdirSync(out, { recursive: true });
const server = await createServer({
  server: { host: '127.0.0.1', port: 1441, strictPort: true, watch: null },
});
const fixture = resolve(`.bailian-protocol-h5-${process.pid}-${Date.now()}.html`);
writeFileSync(
  fixture,
  `<!doctype html><html><head><meta charset="utf-8"></head><body><div id="root"></div><script type="module">
  import React from 'react';
  import {createRoot} from 'react-dom/client';
  import {flushSync} from 'react-dom';
  import {BailianProtocolField} from '/src/pages/settings/BailianProtocolField.tsx';
  import {ProviderFormContext,useProviderForm} from '/src/pages/settings/ProviderForm.tsx';
  import {readCredential,setCredential} from '/src/lib/ipc';
  import i18n from '/src/i18n/index.ts';
  await i18n.changeLanguage('zh-CN');
  const root=createRoot(document.getElementById('root'));
  const change = value => {window.selected=value};
  const blocked = (key,value) => {window.blocked=value};
  function App({channel}) { const form=useProviderForm(); window.finish=()=>form.finish(()=>{window.formClosed=true});
    return React.createElement(ProviderFormContext.Provider,{value:form},React.createElement(BailianProtocolField,{key:channel,channelId:channel,onChange:change,onBlockedChange:blocked})); }
  window.show=channel=>flushSync(()=>root.render(React.createElement(App,{channel})));
  window.read=channel=>readCredential('asr.advanced_config',channel);
  window.write=(channel,value)=>setCredential('asr.advanced_config',value,channel);
  window.choose=value=>{const select=document.querySelector('select');select.value=value;select.dispatchEvent(new Event('change',{bubbles:true}));};
  await window.write('a','{"enableItn":false}');
  window.show('a');window.ready=true;
  </script></body></html>`,
);
await server.listen();
const pageUrl = 'http://127.0.0.1:1441/' + basename(fixture);
const chrome = spawn(
  process.env.CHROME_PATH || 'C:/Program Files/Google/Chrome/Application/chrome.exe',
  [
    '--headless=new',
    '--no-first-run',
    '--no-default-browser-check',
    '--remote-debugging-port=9441',
    '--window-size=900,500',
    '--user-data-dir=' + join(tmpdir(), 'openless-bailian-protocol-' + Date.now()),
    pageUrl,
  ],
  { stdio: 'ignore', windowsHide: true },
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let socket;
try {
  let target;
  for (let i = 0; i < 100; i++) {
    try {
      target = (await (await fetch('http://127.0.0.1:9441/json/list')).json()).find(
        (t) => t.type === 'page' && t.url === pageUrl,
      );
    } catch {}
    if (target) break;
    await sleep(100);
  }
  assert(target, 'Chrome page target');
  socket = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => socket.addEventListener('open', r, { once: true }));
  let id = 0;
  const pending = new Map();
  socket.addEventListener('message', (e) => {
    const m = JSON.parse(String(e.data));
    if (m.id) {
      const p = pending.get(m.id);
      pending.delete(m.id);
      m.error ? p.reject(m.error) : p.resolve(m.result);
    }
  });
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      pending.set(++id, { resolve, reject });
      socket.send(JSON.stringify({ id, method, params }));
    });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails)
      throw Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  };
  for (let i = 0; i < 150; i++) {
    if (await evaluate('Boolean(window.ready)')) break;
    await sleep(100);
  }
  assert(await evaluate('Boolean(window.ready)'), 'React fixture ready');
  const until = async (expression) => {
    for (let i = 0; i < 100; i++) {
      if (await evaluate(expression)) return;
      await sleep(50);
    }
    throw new Error('Timed out: ' + expression);
  };
  await until('!document.querySelector("select").disabled');
  assert.equal(await evaluate('document.querySelector("select").value'), 'auto');
  assert.equal(await evaluate('document.querySelectorAll("option").length'), 6);
  for (const value of [
    'dashscope-realtime',
    'qwen-realtime',
    'multimodal',
    'qwen-multimodal',
    'async-transcription',
  ]) {
    await evaluate(`window.choose(${JSON.stringify(value)});window.finish()`);
    await until('!document.querySelector("select").disabled && !window.blocked');
    assert.equal(JSON.parse(await evaluate('window.read("a")')).bailianProtocol, value);
    assert.equal(JSON.parse(await evaluate('window.read("a")')).enableItn, false);
  }
  await evaluate('window.show("b")');
  await until('!document.querySelector("select").disabled');
  assert.equal(await evaluate('document.querySelector("select").value'), 'auto');
  await evaluate('window.show("a")');
  await until('!document.querySelector("select").disabled');
  assert.equal(await evaluate('document.querySelector("select").value'), 'async-transcription');
  await evaluate('window.choose("auto")');
  await until('!document.querySelector("select").disabled && !window.blocked');
  assert.equal(JSON.parse(await evaluate('window.read("a")')).bailianProtocol, undefined);
  // A malformed configuration must not be overwritten or silently accepted on close.
  await evaluate('window.write("bad","invalid").then(()=>window.show("bad"))');
  await until('Boolean(document.querySelector("[role=alert]"))');
  assert.equal(await evaluate('window.finish()'), false);
  assert.equal(await evaluate('window.read("bad")'), 'invalid');
  console.log(
    'PASS: six options, persistence, channel isolation, auto reset, close/save coordination, invalid configuration',
  );
} finally {
  socket?.close();
  chrome.kill();
  await server.close();
  unlinkSync(fixture);
}
