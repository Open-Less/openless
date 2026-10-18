import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AssistantMarkdown } from '../../src/components/chat/markdown';
import '../../src/styles/tokens.css';

function Fixture() {
  const [draft, setDraft] = useState('');
  const [markdown, setMarkdown] = useState('');
  const [streaming, setStreaming] = useState(false);
  const [mounted, setMounted] = useState(true);
  return (
    <main style={{ padding: 16 }}>
      <label>
        Markdown
        <textarea
          aria-label="Markdown"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
      </label>
      <button
        onClick={() => {
          setMounted(true);
          setStreaming(false);
          setMarkdown(draft);
        }}
      >
        Render
      </button>
      <button
        onClick={() => {
          setMounted(true);
          setStreaming(true);
          setMarkdown(draft);
        }}
      >
        Stream
      </button>
      <button
        onClick={() => {
          setStreaming(false);
          setMarkdown(draft);
        }}
      >
        Finish
      </button>
      <button onClick={() => setMounted(false)}>Unmount</button>
      <button
        onClick={() => {
          document.documentElement.dataset.olTheme = 'dark';
        }}
      >
        Dark
      </button>
      <button
        onClick={() => {
          document.documentElement.dataset.olTheme = 'light';
        }}
      >
        Light
      </button>
      <div id="medium" style={{ fontWeight: 'var(--ol-font-weight-medium)' }}>
        Medium 正文
      </div>
      <div id="semibold" style={{ fontWeight: 'var(--ol-font-weight-semibold)' }}>
        Semibold 标题
      </div>
      <div className="olchat-shell" style={{ padding: 8 }}>
        <span id="light-medium" className="font-medium">
          Fixed light surface
        </span>
      </div>
      <section
        id="answer"
        style={{
          width: '100%',
          maxWidth: 640,
          color: 'var(--ol-ink)',
          background: 'var(--ol-surface)',
        }}
      >
        {mounted && <AssistantMarkdown markdown={markdown} streaming={streaming} />}
      </section>
    </main>
  );
}
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <Fixture />
  </React.StrictMode>,
);
