import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/pages/qa-panel.css';
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
(window as any).__TAURI_INTERNALS__ = { invoke: async () => undefined };
const { QaAnswerFooter } = await import('../../src/components/QaAnswerFooter');
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
