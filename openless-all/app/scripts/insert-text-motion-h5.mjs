import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { createServer } from 'vite';

const out = process.env.OPENLESS_MOTION_ARTIFACT_DIR || join(tmpdir(), 'openless-motion-h5');
mkdirSync(out, { recursive: true });
const server = await createServer({
  server: { host: '127.0.0.1', port: 1438, strictPort: true, watch: null },
});
const fixture = resolve(`.insert-motion-h5-${process.pid}-${Date.now()}.html`);
writeFileSync(
  fixture,
  `<!doctype html><html><head><meta charset="utf-8"><style>
  :root{--ol-font-sans: 'Microsoft YaHei',sans-serif;--ol-capsule-pill-bg:#f4f5f1;--ol-capsule-pill-border:#d5d8d1;--ol-capsule-center-ink:#202624;--ol-capsule-btn-ink:#202624;--ol-capsule-btn-bg:#ddd;--ol-capsule-btn-bg-confirm:#ddd}
  body{margin:0;background:#e2e8e0;display:grid;place-items:center;height:100vh}#root{width:460px}
  </style></head><body><div id="root"></div><script type="module">
  import React from 'react';
  import {createRoot} from 'react-dom/client';
  import {flushSync} from 'react-dom';
  import {LiveTranscriptPill} from '/src/components/LiveTranscriptPill.tsx';
  const root=createRoot(document.getElementById('root'));
  window.show=(text, controls=false, tone='frost')=>flushSync(()=>root.render(React.createElement(LiveTranscriptPill,{text,stageWidth:460,maxWidth:440,minWidth:72,tone,...(controls?{onCancel:()=>{},onConfirm:()=>{}}:{})})));
  window.show('帮我');
  window.ready=true;
  </script></body></html>`,
);
await server.listen();
const pageUrl = 'http://127.0.0.1:1438/' + basename(fixture);
const chrome = spawn(
  process.env.CHROME_PATH || 'C:/Program Files/Google/Chrome/Application/chrome.exe',
  [
    '--headless=new',
    '--no-first-run',
    '--no-default-browser-check',
    '--remote-debugging-port=9437',
    '--window-size=900,500',
    '--user-data-dir=' + join(tmpdir(), 'openless-motion-' + Date.now()),
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
      target = (await (await fetch('http://127.0.0.1:9437/json/list')).json()).find(
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
  await sleep(800);
  const samples = await evaluate(`new Promise(resolve=>{
    const frames=[]; const start=performance.now();window.show('帮我查找一下');
    function tick(now){const shell=document.querySelector('.ol-live-transcript-pill').getBoundingClientRect();
    const glyphs=[...document.querySelectorAll('.ol-live-transcript-position')].map(e=>{const c=e.firstElementChild;const s=getComputedStyle(c);const r=e.getBoundingClientRect();return {text:e.textContent,x:r.x,y:new DOMMatrix(s.transform).m42,opacity:+s.opacity};});
    frames.push({t:now-start,left:shell.x,width:shell.width,glyphs});if(now-start<1200)requestAnimationFrame(tick);else resolve(frames);}requestAnimationFrame(tick);
  })`);
  writeFileSync(join(out, 'trajectory.json'), JSON.stringify(samples, null, 2));
  assert(samples.length > 15, 'animation sampled across frames');
  const settledGap = samples.at(-1).glyphs[2].x - samples.at(-1).glyphs[1].x;
  assert(
    samples
      .filter((frame) => frame.glyphs[2].opacity > 0.25)
      .every((frame) => frame.glyphs[2].x - frame.glyphs[1].x > settledGap * 0.7),
    'new glyph cannot appear on top of the retained tail',
  );
  assert(
    samples.some((f) => f.glyphs[2].y > 1),
    'new glyph rises from below',
  );
  assert(
    samples.some((f) => f.glyphs[2].y < -0.05),
    'new glyph gently overshoots baseline',
  );
  const settled = samples.at(-1);
  assert(Math.abs(settled.glyphs[2].y) < 0.1, 'glyph settles');
  assert(
    settled.glyphs.every((g) => g.opacity > 0.99),
    'glyphs opaque',
  );
  const center = await evaluate(
    `(()=>{const r=document.getElementById('root').getBoundingClientRect();return r.x+r.width/2})()`,
  );
  assert(Math.abs(settled.left + settled.width / 2 - center) < 0.2, 'shell recenters');
  assert(
    samples.some((frame) => frame.left + frame.width / 2 < center - 8),
    'shell visibly leans left before recentering',
  );
  await evaluate(`window.show('帮我查找上海的天气')`);
  await sleep(1200);
  const wave = await evaluate(`new Promise(resolve => {
    const read = () => [...document.querySelectorAll('.ol-live-transcript-position')].slice(0,9).map(e => new DOMMatrix(getComputedStyle(e).transform).m41);
    const before=read();const start=performance.now();const frames=[];
    window.show('帮我查找上海的天气预报吧');
    function tick(now){frames.push({t:now-start,delta:read().map((x,i)=>x-before[i])});
      if(now-start<800)requestAnimationFrame(tick);else resolve(frames);}
    requestAnimationFrame(tick);
  })`);
  const onset = (index) => wave.find((frame) => frame.delta[index] < -0.4)?.t;
  assert(onset(5) > onset(8) + 12, 'next group begins after the rightmost group');
  assert(Math.abs(onset(6) - onset(8)) < 16, 'three glyphs begin together');
  assert(
    wave.some((frame) => frame.delta[8] < -8 && frame.delta[5] < -2 && frame.delta[0] < -0.5),
    'multiple groups move concurrently',
  );
  writeFileSync(join(out, 'wave.json'), JSON.stringify(wave, null, 2));
  await evaluate(`window.show('帮我查找一下10六号')`);
  await sleep(45);
  await evaluate(`window.show('帮我查找一下10六号的天气')`);
  await sleep(45);
  await evaluate(`window.show('帮我查找一下十六号的天气')`);
  await sleep(900);
  assert.equal(
    await evaluate(`document.querySelector('[role=status]').textContent`),
    '帮我查找一下十六号的天气',
  );
  await evaluate(`window.show('长文本测试'.repeat(25),true,'dark')`);
  await sleep(1000);
  const clip = await evaluate(
    `(()=>{const t=document.querySelector('.ol-live-transcript-track');const p=document.querySelector('.ol-live-transcript-pill');const r=t.getBoundingClientRect();const b=p.getBoundingClientRect();return {overflow:getComputedStyle(t).overflow,width:b.width,left:r.left-b.left,right:b.right-r.right,count:document.querySelectorAll('button').length}})()`,
  );
  assert.equal(clip.overflow, 'hidden');
  assert(clip.width <= 440.2);
  assert(clip.left >= 45 && clip.right >= 45);
  assert.equal(clip.count, 2);
  await send('Emulation.setEmulatedMedia', {
    features: [{ name: 'prefers-reduced-motion', value: 'reduce' }],
  });
  await send('Page.reload');
  await sleep(1000);
  await evaluate(`window.show('减少动态效果验证')`);
  await sleep(80);
  assert(
    await evaluate(
      `[...document.querySelectorAll('.ol-live-transcript-char')].every(e=>+getComputedStyle(e).opacity===1&&Math.abs(new DOMMatrix(getComputedStyle(e).transform).m42)<0.01)`,
    ),
    'reduced motion immediate',
  );
  await send('Emulation.setEmulatedMedia', { features: [] });
  await send('Page.reload');
  await sleep(900);
  await evaluate(`window.show('帮我查找一下十六号的天气')`);
  await sleep(1000);
  // Optional real browser filmstrip; timestamps preserve capture cadence.
  if (process.env.OPENLESS_MOTION_RECORD === '1') {
    const frames = [];
    socket.addEventListener('message', (event) => {
      const message = JSON.parse(String(event.data));
      if (message.method !== 'Page.screencastFrame') return;
      const file = `motion-${String(frames.length).padStart(4, '0')}.jpg`;
      writeFileSync(join(out, file), Buffer.from(message.params.data, 'base64'));
      frames.push({ file, time: message.params.metadata.timestamp });
      void send('Page.screencastFrameAck', { sessionId: message.params.sessionId });
    });
    await evaluate(`window.show('')`);
    await sleep(600);
    await send('Page.startScreencast', { format: 'jpeg', quality: 90, everyNthFrame: 1 });
    for (const [text, pause] of [
      ['帮我', 900],
      ['帮我查找一', 650],
      ['帮我查找一下', 650],
      ['帮我查找一下10六号', 800],
      ['帮我查找一下，16号', 1000],
    ]) {
      await evaluate(`window.show(${JSON.stringify(text)})`);
      await sleep(pause);
    }
    await send('Page.stopScreencast');
    writeFileSync(
      join(out, 'motion.ffconcat'),
      'ffconcat version 1.0\n' +
        frames
          .map(
            (frame, i) =>
              `file '${frame.file}'\nduration ${Math.max(0.001, (frames[i + 1]?.time ?? frame.time + 0.5) - frame.time)}\n`,
          )
          .join('') +
        `file '${frames.at(-1).file}'\n`,
    );
  }
  const shot = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(out, 'settled.png'), Buffer.from(shot.data, 'base64'));
  await send('Page.navigate', { url: pageUrl });
  await sleep(1200);
  await evaluate(`window.show('10六号')`);
  await sleep(1000);
  const correction = await evaluate(`new Promise(resolve=>{
    const frames=[];const start=performance.now();window.show('十六号');
    function tick(now){const glyphs=[...document.querySelectorAll('.ol-live-transcript-position')].map(e=>({x:e.getBoundingClientRect().x,width:e.getBoundingClientRect().width,opacity:+getComputedStyle(e.firstElementChild).opacity}));frames.push(glyphs);
      if(now-start<600)requestAnimationFrame(tick);else resolve(frames);}
    requestAnimationFrame(tick);
  })`);
  assert(
    correction.every(
      (frame) => frame[0].opacity < 0.1 || frame[0].x + frame[0].width <= frame[1].x + 1,
    ),
    'middle correction never crosses retained suffix',
  );
  // Exercise the production Capsule route, not just the isolated animation.
  for (const style of ['siri', 'classic', 'typeless']) {
    for (const enabled of [true, false]) {
      await send('Emulation.setDeviceMetricsOverride', {
        width: style === 'typeless' ? 206 : 460,
        height: style === 'typeless' ? 57 : style === 'classic' ? 100 : 180,
        deviceScaleFactor: 1,
        mobile: false,
      });
      await send('Page.navigate', {
        url: `http://127.0.0.1:1438/?window=capsule&os=win&style=${style}&insertDemo=1&transcript=${enabled ? 1 : 0}&fontSize=20`,
      });
      // Production demo simulates five recognition batches ending after 2440ms.
      for (let attempt = 0; attempt < 150; attempt += 1) {
        const mounted = await evaluate(
          `Boolean(document.querySelector('.ol-live-transcript-pill, .ol-typeless-capsule, .ol-capsule-pill, canvas'))`,
        );
        if (mounted) break;
        await sleep(100);
      }
      await sleep(3200);
      const result = await evaluate(`(() => {
        const pills=[...document.querySelectorAll('.ol-live-transcript-pill')];
        const chars=[...document.querySelectorAll('.ol-live-transcript-char')];
        const rect=pills[0]?.getBoundingClientRect();
        return {pills:pills.length,chars:chars.length,width:rect?.width,font:chars[0]?parseFloat(getComputedStyle(chars[0]).fontSize):0,buttons:document.querySelectorAll('button').length};
      })()`);
      if (result.pills !== (enabled ? 1 : 0)) {
        writeFileSync(
          join(out, 'integration-failure.txt'),
          await evaluate('document.documentElement.outerHTML'),
        );
      }
      assert.equal(result.pills, enabled ? 1 : 0, `${style} display preference`);
      if (enabled) {
        assert(result.chars > 0, `${style} original text`);
        assert(result.width <= (style === 'typeless' ? 206 : 460), `${style} bounded width`);
        assert(
          Math.abs(result.font * (style === 'typeless' ? 0.447 : 1) - 20) < 0.1,
          `${style} visible font size`,
        );
      }
      if (style !== 'siri') assert(result.buttons >= 2, `${style} keeps controls`);
      const shot = await send('Page.captureScreenshot', { format: 'png' });
      writeFileSync(
        join(out, `${style}-${enabled ? 'text' : 'wave'}.png`),
        Buffer.from(shot.data, 'base64'),
      );
    }
  }
  console.log(
    JSON.stringify(
      {
        passed: true,
        frames: samples.length,
        minY: Math.min(...samples.map((f) => f.glyphs[2].y)),
        clip,
        artifacts: resolve(out),
      },
      null,
      2,
    ),
  );
} finally {
  socket?.close();
  chrome.kill();
  await server.close();
  unlinkSync(fixture);
}
