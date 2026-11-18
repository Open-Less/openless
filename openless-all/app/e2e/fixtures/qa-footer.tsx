import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/features/qa/qa-panel.css';
const tauriConfig = await import('../../src-tauri/tauri.conf.json');
const policy = document.createElement('meta');
policy.httpEquiv = 'Content-Security-Policy';
policy.content = `img-src ${tauriConfig.default.app.security.csp['img-src']}`;
document.head.appendChild(policy);
const i18n = createInstance();
await i18n.init({
  lng: 'zh-CN',
  resources: { 'zh-CN': { translation: { common: { copy: '复制' }, qa: { sources: '来源' } } } },
});
const playback: Array<{ command: string; requestId: string }> = [];
let resumeAudio: (() => void) | undefined;
let finishAudio: (() => void) | undefined;
let holdResume = false;
Object.assign(window, {
  qaFooterFixture: {
    playback,
    holdResume: () => {
      holdResume = true;
    },
    releaseResume: () => resumeAudio?.(),
    finish: () => finishAudio?.(),
  },
  AudioContext: class {
    currentTime = 0;
    resume() {
      if (!holdResume) return Promise.resolve();
      return new Promise<void>((resolve) => {
        resumeAudio = resolve;
      });
    }
    close() {
      return Promise.resolve();
    }
  },
  __TAURI_INTERNALS__: {
    transformCallback: () => 1,
    unregisterCallback: () => undefined,
    async invoke(command: string, args?: { requestId: string }) {
      if (command === 'get_startup_snapshot')
        return { contractVersion: '2.0.0', backend: { running: true } };
      if (command === 'deepseek_tts_start' || command === 'deepseek_tts_stop') {
        playback.push({ command, requestId: args!.requestId });
        if (command === 'deepseek_tts_start')
          await new Promise<void>((resolve) => {
            finishAudio = resolve;
          });
      }
    },
  },
});
const { QaAnswerFooter } = await import('../../src/features/qa/QaAnswerFooter');
function Fixture() {
  const [mode, setMode] = useState('traditional');
  return (
    <I18nextProvider i18n={i18n}>
      <main style={{ padding: 32, width: 640 }}>
        <div>
          {['traditional', 'multimodal', 'deepseek'].map((value) => (
            <button key={value} onClick={() => setMode(value)}>
              {value}
            </button>
          ))}
        </div>
        <article className="qa-assistant-message" style={{ marginTop: 24 }}>
          <p>这是回答的第一段。</p>
          <p>这是回答的最后一段。</p>
          <QaAnswerFooter
            text="测试回答"
            canReadAloud={mode === 'deepseek'}
            sources={[
              { url: 'https://example.com/a', host: 'example.com', title: '来源一' },
              { url: 'https://example.org/b', host: 'example.org', title: '来源二' },
            ]}
          />
        </article>
      </main>
    </I18nextProvider>
  );
}
createRoot(document.getElementById('root')!).render(<Fixture />);
