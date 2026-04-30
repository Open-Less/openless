// History.tsx — 接 Tauri 后端 list_history / delete_history_entry / clear_history。
// 真实数据来自 ~/Library/Application Support/OpenLess/history.json。

import { useEffect, useMemo, useState } from 'react';
import { Icon } from '../components/Icon';
import { detectOS } from '../components/WindowChrome';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import { clearHistory, deleteHistoryEntry, listHistory } from '../lib/ipc';
import type { DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

const FILTERS: Array<{ id: 'all' | PolishMode; label: string }> = [
  { id: 'all', label: '全部' },
  { id: 'raw', label: '原文' },
  { id: 'light', label: '轻度润色' },
  { id: 'structured', label: '清晰结构' },
  { id: 'formal', label: '正式表达' },
];

const MODE_LABEL: Record<PolishMode, string> = {
  raw: '原文',
  light: '轻度润色',
  structured: '清晰结构',
  formal: '正式表达',
};

export function History() {
  const [filter, setFilter] = useState<'all' | PolishMode>('all');
  const [items, setItems] = useState<DictationSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const { hotkey } = useHotkeySettings();

  const refresh = async () => {
    const data = await listHistory();
    setItems(data);
    setLoading(false);
    if (data.length > 0 && !selectedId) {
      setSelectedId(data[0].id);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const filtered = useMemo(
    () => (filter === 'all' ? items : items.filter(s => s.mode === filter)),
    [items, filter],
  );
  const item = useMemo(
    () => filtered.find(s => s.id === selectedId) || filtered[0],
    [filtered, selectedId],
  );

  const onClear = async () => {
    if (items.length === 0) return;
    if (!confirm(`确定清空全部 ${items.length} 条记录？此操作不可恢复。`)) return;
    await clearHistory();
    setItems([]);
    setSelectedId(null);
  };

  const onDelete = async () => {
    if (!item) return;
    await deleteHistoryEntry(item.id);
    setItems(prev => prev.filter(s => s.id !== item.id));
  };

  const onCopy = () => {
    if (!item) return;
    navigator.clipboard?.writeText(item.finalText);
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <PageHeader
        kicker="HISTORY"
        title="历史记录"
        desc="最近的识别结果只保存在本机。左侧为时间线，右侧为原文与润色对比。"
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={refresh}>刷新</Btn>
            <Btn icon="trash" variant="ghost" size="sm" onClick={onClear}>清空</Btn>
          </div>
        }
      />
      <div style={{ display: 'grid', gridTemplateColumns: '300px 1fr', gap: 14, flex: 1, minHeight: 0 }}>
        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ padding: '12px 14px', borderBottom: '0.5px solid var(--ol-line)' }}>
            <div style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '6px 10px', fontSize: 12,
              border: '0.5px solid var(--ol-line-strong)', borderRadius: 8,
              background: 'var(--ol-surface-2)', color: 'var(--ol-ink-3)',
            }}>
              <Icon name="search" size={12} />
              <span style={{ flex: 1 }}>共 {items.length} 条 · 显示 {filtered.length}</span>
            </div>
            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 10 }}>
              {FILTERS.map(f => (
                <button
                  key={f.id}
                  onClick={() => setFilter(f.id)}
                  style={{
                    padding: '3px 9px', fontSize: 11, borderRadius: 999,
                    border: '0.5px solid ' + (filter === f.id ? 'var(--ol-ink)' : 'var(--ol-line-strong)'),
                    background: filter === f.id ? 'var(--ol-ink)' : 'transparent',
                    color: filter === f.id ? '#fff' : 'var(--ol-ink-3)',
                    cursor: 'default', fontFamily: 'inherit', fontWeight: 500,
                  }}
                >{f.label}</button>
              ))}
            </div>
          </div>
          <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
            {loading && <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>加载中…</div>}
            {!loading && filtered.length === 0 && (
              <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
                还没有历史记录。按 {getHotkeyTriggerLabel(hotkey?.trigger)} 录一段试试。
              </div>
            )}
            {filtered.map(s => (
              <button
                key={s.id}
                onClick={() => setSelectedId(s.id)}
                style={{
                  width: '100%', padding: '10px 12px', textAlign: 'left',
                  display: 'flex', flexDirection: 'column', gap: 4,
                  border: 0, borderRadius: 8,
                  background: selectedId === s.id ? 'rgba(37,99,235,0.06)' : 'transparent',
                  boxShadow: selectedId === s.id ? 'inset 2px 0 0 var(--ol-blue)' : 'none',
                  cursor: 'default', fontFamily: 'inherit', marginBottom: 1,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                  <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
                    {formatTime(s.createdAt)}
                  </span>
                  <span style={{ fontSize: 10, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
                    {formatDuration(s.durationMs)}
                  </span>
                </div>
                <div style={{ fontSize: 12, color: 'var(--ol-ink-2)', lineHeight: 1.45, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                  {s.finalText.split('\n')[0]}
                </div>
                <div><Pill size="sm" tone={s.mode === 'raw' ? 'outline' : 'default'}>{MODE_LABEL[s.mode]}</Pill></div>
              </button>
            ))}
          </div>
        </Card>

        <Card padding={20} style={{ overflow: 'auto' }}>
          {item ? (
            <>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 13, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{formatTime(item.createdAt)}</span>
                  <Pill size="sm" tone="default">{MODE_LABEL[item.mode]}</Pill>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{formatDuration(item.durationMs)}</span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <Btn icon="copy" variant="ghost" size="sm" onClick={onCopy}>复制</Btn>
                  <Btn icon="trash" variant="ghost" size="sm" onClick={onDelete}>删除</Btn>
                </div>
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-line)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
                  <Pill size="sm" tone="outline" style={{ marginBottom: 10 }}>原文</Pill>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)', whiteSpace: 'pre-wrap' }}>
                    {item.rawTranscript || '（空）'}
                  </p>
                </div>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-blue)', borderRadius: 10, background: 'var(--ol-blue-soft)' }}>
                  <Pill size="sm" tone="blue" style={{ marginBottom: 10 }}>{MODE_LABEL[item.mode]}</Pill>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)', whiteSpace: 'pre-line' }}>
                    {item.finalText}
                  </p>
                </div>
              </div>
              <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 18, fontSize: 11, color: 'var(--ol-ink-4)', flexWrap: 'wrap' }}>
                {item.appName && <span>插入到 <b style={{ color: 'var(--ol-ink-2)' }}>{item.appName}</b></span>}
                <span>{item.finalText.length} 字</span>
                {item.dictionaryEntryCount != null && item.dictionaryEntryCount > 0 && (
                  <span>{item.dictionaryEntryCount} 个热词</span>
                )}
                <span>{item.insertStatus === 'inserted' ? '已插入' : item.insertStatus === 'copiedFallback' ? `已复制(需 ${detectOS() === 'win' ? 'Ctrl+V' : '⌘V'})` : '插入失败'}</span>
              </div>
            </>
          ) : (
            <div style={{ padding: 40, textAlign: 'center', fontSize: 13, color: 'var(--ol-ink-4)' }}>
              {loading ? '加载中…' : '左侧选一条查看详情。'}
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const pad = (n: number) => String(n).padStart(2, '0');
  if (sameDay) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return `${d.getMonth() + 1}/${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function formatDuration(ms: number | null): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return `${sec.toFixed(1)}s`;
  return `${(sec / 60).toFixed(1)}m`;
}
