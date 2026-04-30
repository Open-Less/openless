// Vocab.tsx — 接 Tauri 后端 list_vocab / add_vocab / remove_vocab / set_vocab_enabled。
// 数据落地到 ~/Library/Application Support/OpenLess/dictionary.json（与 Swift 同名）。

import { useEffect, useRef, useState } from 'react';
import { addVocab, listVocab, removeVocab, setVocabEnabled } from '../lib/ipc';
import type { DictionaryEntry } from '../lib/types';
import { Btn, Card, PageHeader } from './_atoms';

export function Vocab() {
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const inputRef = useRef<HTMLInputElement>(null);

  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      setError(null);
      const data = await listVocab();
      setEntries(data);
    } catch (e) {
      // 之前没 try/catch,后端 decode 失败时 spinner 永久卡死。
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const onAdd = async () => {
    const phrase = inputRef.current?.value.trim();
    if (!phrase) return;
    await addVocab(phrase);
    if (inputRef.current) inputRef.current.value = '';
    refresh();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      void onAdd();
    }
  };

  const onRemove = async (id: string) => {
    await removeVocab(id);
    setEntries(prev => prev.filter(e => e.id !== id));
  };

  const onToggle = async (entry: DictionaryEntry) => {
    const next = !entry.enabled;
    setEntries(prev => prev.map(e => (e.id === entry.id ? { ...e, enabled: next } : e)));
    await setVocabEnabled(entry.id, next);
  };

  return (
    <>
      <PageHeader
        kicker="VOCABULARY"
        title="词汇表"
        desc="告诉模型识别前可能出现的词——生词、新词或专业词汇。同时进入 ASR 热词与后期模型上下文。"
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={refresh}>刷新</Btn>
          </div>
        }
      />
      <Card padding={0}>
        <div style={{ padding: 18, borderBottom: '0.5px solid var(--ol-line)' }}>
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              ref={inputRef}
              placeholder="输入词语，按 Enter 或点添加…"
              onKeyDown={onKeyDown}
              style={{
                flex: 1, height: 36, padding: '0 12px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8, fontSize: 13,
                fontFamily: 'inherit', outline: 'none',
                background: 'var(--ol-surface-2)',
              }}
            />
            <Btn variant="primary" icon="plus" onClick={onAdd}>添加</Btn>
          </div>
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 10 }}>
            支持中英混合 · 数字开头按字面识别 · 命中次数自动计数
          </div>
        </div>
        <div style={{ padding: 18, display: 'flex', flexWrap: 'wrap', gap: 8, minHeight: 80 }}>
          {loading && <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>加载中…</div>}
          {!loading && error && (
            <div style={{ fontSize: 12, color: 'var(--ol-err)', lineHeight: 1.6 }}>
              加载失败:{error}
            </div>
          )}
          {!loading && !error && entries.length === 0 && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>
              还没有词条。在上面输入一个生词或专业术语，让模型在听写时优先匹配。
            </div>
          )}
          {!error && entries.map(e => (
            <VocabChip key={e.id} entry={e} onRemove={() => onRemove(e.id)} onToggle={() => onToggle(e)} />
          ))}
        </div>
      </Card>
    </>
  );
}

interface VocabChipProps {
  entry: DictionaryEntry;
  onRemove: () => void;
  onToggle: () => void;
}

function VocabChip({ entry, onRemove, onToggle }: VocabChipProps) {
  const enabled = entry.enabled;
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '5px 10px 5px 12px',
        borderRadius: 999,
        border: '0.5px solid var(--ol-line-strong)',
        background: enabled ? (entry.hits > 0 ? 'var(--ol-blue-soft)' : 'var(--ol-surface)') : 'var(--ol-surface-2)',
        opacity: enabled ? 1 : 0.55,
        fontSize: 12, color: 'var(--ol-ink)',
        fontFamily: 'var(--ol-font-mono)',
      }}
    >
      <button
        onClick={onToggle}
        title={enabled ? '点击禁用此词条' : '点击启用此词条'}
        style={{ background: 'transparent', border: 0, padding: 0, color: 'inherit', fontFamily: 'inherit', cursor: 'default' }}
      >
        {entry.phrase}
      </button>
      <span
        style={{
          minWidth: 18, height: 18, padding: '0 5px',
          borderRadius: 999, fontSize: 10, fontWeight: 600,
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          background: entry.hits > 0 && enabled ? 'var(--ol-blue)' : 'rgba(0,0,0,0.06)',
          color: entry.hits > 0 && enabled ? '#fff' : 'var(--ol-ink-4)',
          fontFamily: 'var(--ol-font-sans)',
        }}
      >{entry.hits}</span>
      <button
        onClick={onRemove}
        aria-label="删除"
        style={{
          width: 14, height: 14, padding: 0, border: 0, borderRadius: 999,
          background: 'transparent', color: 'var(--ol-ink-4)',
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'default',
        }}
      >
        <svg width="8" height="8" viewBox="0 0 8 8"><path d="M1 1l6 6M7 1l-6 6" stroke="currentColor" strokeWidth="1.4" /></svg>
      </button>
    </span>
  );
}
