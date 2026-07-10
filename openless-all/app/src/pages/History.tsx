// History.tsx — 接 Tauri 后端 list_history / delete_history_entry / clear_history。
// 真实数据来自 ~/Library/Application Support/OpenLess/history.json。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { detectOS } from '../components/WindowChrome';
import { formatComboLabel } from '../lib/hotkey';
import { clearHistory, deleteHistoryEntry, listHistory, readAudioRecording, retranscribeRecording } from '../lib/ipc';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';
import { chipSelectedStyle } from './settings/shared';

function useFilters(): Array<{ id: 'all' | PolishMode; label: string }> {
  const { t } = useTranslation();
  return [
    { id: 'all', label: t('history.filterAll') },
    { id: 'raw', label: t('style.modes.raw.name') },
    { id: 'light', label: t('style.modes.light.name') },
    { id: 'structured', label: t('style.modes.structured.name') },
    { id: 'formal', label: t('style.modes.formal.name') },
  ];
}

function useModeLabel(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

export function History() {
  const { t } = useTranslation();
  const os = detectOS();
  const FILTERS = useFilters();
  const MODE_LABEL = useModeLabel();
  const [filter, setFilter] = useState<'all' | PolishMode>('all');
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [items, setItems] = useState<DictationSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [justCopied, setJustCopied] = useState(false);
  const [justCopiedRaw, setJustCopiedRaw] = useState(false);
  // 「重新转录」进行中：禁用按钮 + 显示「转录中…」，避免重复点击发起多次 ASR。
  const [retranscribing, setRetranscribing] = useState(false);
  // 录音文件 lazily-detected missing 状态：retention / 条数 cap 清理后磁盘上 wav
  // 可能已被删，但 history 条目 hasAudioRecording 仍写 true。任一组件
  // （播放 / 导出）首次 IPC 拿到 'recording not found' 时把 id 加进来，
  // 之后渲染按钮的条件就转 false，避免反复点击得到同样的 error。
  // 修 pr_agent "Missing file check" 反馈。
  const [audioMissingIds, setAudioMissingIds] = useState<Set<string>>(() => new Set());
  const markAudioMissing = useCallback((id: string) => {
    setAudioMissingIds(prev => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);
  const { prefs } = useHotkeySettings();
  const mobile = useMobileLayout();
  const [mobileDetailOpen, setMobileDetailOpen] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listHistory();
      setItems(data);
      setActionError(null);
      setSelectedId(prev => (prev && data.some(s => s.id === prev) ? prev : data[0]?.id ?? null));
    } catch (error) {
      console.error('[history] failed to load history', error);
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const searchShortcut = os === 'mac' ? '⌘K' : 'Ctrl+K';

  // 搜索词防抖：随输入实时更新 query，300ms 后落到 debouncedQuery 再过滤，
  // 避免每个按键都重算整张列表（与 Marketplace 同模式）。
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(id);
  }, [query]);

  // ⌘K / Ctrl+K 聚焦搜索框（设计稿提示的快捷键）。
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        searchInputRef.current?.focus();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const filtered = useMemo(() => {
    const byMode = filter === 'all' ? items : items.filter(s => s.mode === filter);
    const q = debouncedQuery.trim().toLowerCase();
    if (!q) return byMode;
    // 按原始转写 + 润色后文本匹配关键词，覆盖用户能想起的两种内容。
    return byMode.filter(
      s =>
        s.rawTranscript.toLowerCase().includes(q) ||
        s.finalText.toLowerCase().includes(q),
    );
  }, [items, filter, debouncedQuery]);
  const item = useMemo(
    () => filtered.find(s => s.id === selectedId) || filtered[0],
    [filtered, selectedId],
  );

  const onClear = async () => {
    if (items.length === 0) return;
    if (!confirm(t('history.confirmClear', { count: items.length }))) return;
    setActionError(null);
    try {
      await clearHistory();
      setItems([]);
      setSelectedId(null);
    } catch (error) {
      console.error('[history] failed to clear history', error);
      setActionError(t('history.clearFailed', { err: errorMessage(error) }));
    }
  };

  const onDelete = async () => {
    if (!item) return;
    const deletedId = item.id;
    setActionError(null);
    try {
      await deleteHistoryEntry(deletedId);
      setItems(prev => prev.filter(s => s.id !== deletedId));
      setSelectedId(current => (current === deletedId ? null : current));
    } catch (error) {
      console.error('[history] failed to delete history entry', error);
      setActionError(t('history.deleteFailed', { err: errorMessage(error) }));
    }
  };

  const onCopy = async () => {
    if (!item) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('clipboard unavailable');
      }
      // 润色失败/未产出时 finalText 为空，回退到原文，避免「复制」按钮复制空字符串
      // 导致原文无法从 UI 取回（polish 失败时仍能拿到识别原文）。
      await navigator.clipboard.writeText(item.finalText.trim() ? item.finalText : item.rawTranscript);
      setActionError(null);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy entry', error);
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  // 原文（识别结果）单独复制：润色失败或用户只想要未润色文本时使用。
  const onCopyRaw = async () => {
    if (!item) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('clipboard unavailable');
      }
      await navigator.clipboard.writeText(item.rawTranscript);
      setActionError(null);
      setJustCopiedRaw(true);
      window.setTimeout(() => setJustCopiedRaw(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy raw transcript', error);
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  const onExportAudio = async () => {
    if (!item || !item.hasAudioRecording) return;
    try {
      const bytes = await readAudioRecording(item.id);
      if (bytes.byteLength === 0) throw new Error('empty recording');
      const buffer = new ArrayBuffer(bytes.byteLength);
      new Uint8Array(buffer).set(bytes);
      const blob = new Blob([buffer], { type: 'audio/wav' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `openless-recording-${item.id}.wav`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      // 浏览器异步触发下载，立刻 revoke 偶尔被中断；延后 60s 兜底。
      window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
      setActionError(null);
    } catch (error) {
      console.error('[history] failed to export recording', error);
      const msg = errorMessage(error);
      // wav 已被 retention / 条数 cap 清理：把按钮隐藏，不显示错误（用户没干错事）。
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(item.id);
        return;
      }
      setActionError(t('history.exportFailed', { err: msg }));
    }
  };

  // 对一条「转录失败 / 没识别到语音」的历史用当前 ASR provider 重新转录（issue #613）。
  // 后端读 recordings/<id>.wav → 重转 → 原地回写该条 rawTranscript/finalText、清 errorCode，
  // 返回整条记录；前端据此局部刷新。失败保留 + 自动重试已让这些条目的录音留得住，这里给
  // 持久失败（重试也没救回来）一个手动重转入口。
  const onRetranscribe = async () => {
    if (!item || !item.hasAudioRecording) return;
    setRetranscribing(true);
    setActionError(null);
    try {
      const updated = await retranscribeRecording(item.id);
      setItems(prev => prev.map(s => (s.id === updated.id ? updated : s)));
    } catch (error) {
      console.error('[history] retranscribe failed', error);
      const msg = errorMessage(error);
      // wav 已被 retention / 条数 cap 清理：隐藏入口，不报错（用户没干错事）。
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(item.id);
        return;
      }
      setActionError(t('history.retranscribeFailed', { err: msg }));
    } finally {
      setRetranscribing(false);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <PageHeader
        kicker={t('history.kicker')}
        title={t('history.title')}
        desc={t('history.desc')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>{t('common.refresh')}</Btn>
            <Btn icon="trash" variant="ghost" size="sm" onClick={onClear}>{t('common.clear')}</Btn>
          </div>
        }
      />
      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '300px 1fr', gap: 14, flex: 1, minHeight: 0 }}>
        {( !mobile || !mobileDetailOpen) && (
        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ padding: '12px 14px', borderBottom: '0.5px solid var(--ol-line)' }}>
            <div style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '6px 10px', fontSize: 12,
              border: '0.5px solid var(--ol-line-strong)', borderRadius: 8,
              background: 'var(--ol-surface-2)', color: 'var(--ol-ink-3)',
            }}>
              <Icon name="search" size={12} />
              <input
                ref={searchInputRef}
                type="search"
                value={query}
                onChange={e => setQuery(e.target.value)}
                placeholder={t('history.searchPlaceholder', { shortcut: searchShortcut })}
                aria-label={t('history.searchPlaceholder', { shortcut: searchShortcut })}
                style={{
                  flex: 1, minWidth: 0,
                  outline: 'none', border: 0, background: 'transparent',
                  fontSize: 12, color: 'var(--ol-ink-1)', fontFamily: 'inherit',
                }}
              />
            </div>
            <div style={{ marginTop: 6, fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {t('history.summary', { total: items.length, shown: filtered.length })}
            </div>
            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 10 }}>
              {FILTERS.map(f => (
                <button
                  key={f.id}
                  onClick={() => setFilter(f.id)}
                  style={{
                    padding: '3px 9px', fontSize: 11, borderRadius: 999,
                    ...chipSelectedStyle(filter === f.id),
                    cursor: 'default', fontFamily: 'inherit', fontWeight: 500,
                    transition: 'background 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
                  }}
                >{f.label}</button>
              ))}
            </div>
          </div>
          <div className="ol-thinscroll" style={{ flex: 1, overflow: 'auto', padding: 6 }}>
            {actionError && (
              <div style={{ margin: 8, padding: '9px 10px', borderRadius: 8, background: 'rgba(239,68,68,0.08)', color: 'var(--ol-red, #ef4444)', fontSize: 12, lineHeight: 1.45 }}>
                {actionError}
              </div>
            )}
            {loading && <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>}
            {!loading && loadError && (
              <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)', display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 10 }}>
                <span>{t('history.loadFailed', { err: loadError })}</span>
                <Btn size="sm" variant="ghost" onClick={() => void refresh()}>{t('history.retry')}</Btn>
              </div>
            )}
            {!loading && !loadError && filtered.length === 0 && (
              <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
                {debouncedQuery.trim()
                  ? t('history.searchNoMatch', { query: debouncedQuery.trim() })
                  : t('history.empty', { trigger: prefs ? formatComboLabel(prefs.dictationHotkey) : '' })}
              </div>
            )}
            {!loadError && filtered.map(s => (
              <button
                key={s.id}
                onClick={() => {
                  setSelectedId(s.id);
                  if (mobile) setMobileDetailOpen(true);
                }}
                style={{
                  width: '100%', padding: '10px 12px', textAlign: 'left',
                  display: 'flex', flexDirection: 'column', gap: 4,
                  border: 0, borderRadius: 8,
                  background: selectedId === s.id ? 'rgba(37,99,235,0.06)' : 'transparent',
                  boxShadow: selectedId === s.id ? 'inset 2px 0 0 var(--ol-blue)' : 'none',
                  cursor: 'default', fontFamily: 'inherit', marginBottom: 1,
                  transition: 'background 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft)',
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                  <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
                    {formatTime(s.createdAt)}
                  </span>
                  <span style={{ fontSize: 10, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
                    {formatDuration(s.durationMs, t)}
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
        )}

        {(!mobile || mobileDetailOpen) && (
        <Card padding={20} className="ol-thinscroll" style={{ overflow: 'auto' }}>
          {item ? (
            <>
              {mobile && (
                <div style={{ marginBottom: 12 }}>
                  <Btn icon="chevLeft" variant="ghost" size="sm" onClick={() => setMobileDetailOpen(false)}>
                    {t('history.backToList')}
                  </Btn>
                </div>
              )}
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14, flexWrap: 'wrap', gap: 8 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 13, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{formatTime(item.createdAt)}</span>
                  <Pill size="sm" tone="default">{MODE_LABEL[item.mode]}</Pill>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{formatDuration(item.durationMs, t)}</span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <Btn icon={justCopied ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopy()}>{justCopied ? t('common.copied') : t('common.copy')}</Btn>
                  {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                    <Btn icon="download" variant="ghost" size="sm" onClick={() => void onExportAudio()}>{t('history.exportRecording')}</Btn>
                  )}
                  {item.hasAudioRecording
                    && !audioMissingIds.has(item.id)
                    && (item.errorCode === 'transcribeFailed' || item.errorCode === 'emptyTranscript') && (
                    <Btn icon="refresh" variant="ghost" size="sm" disabled={retranscribing} onClick={() => void onRetranscribe()}>
                      {retranscribing ? t('history.retranscribing') : t('history.retranscribe')}
                    </Btn>
                  )}
                  <Btn icon="trash" variant="ghost" size="sm" onClick={onDelete}>{t('common.delete')}</Btn>
                </div>
              </div>
              {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                <AudioRecordingPlayer
                  sessionId={item.id}
                  onMissing={() => markAudioMissing(item.id)}
                  key={item.id}
                />
              )}
              <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12 }}>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-line)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 10 }}>
                    <Pill size="sm" tone="outline">{t('history.rawLabel')}</Pill>
                    {item.rawTranscript && (
                      <Btn icon={justCopiedRaw ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopyRaw()}>
                        {justCopiedRaw ? t('common.copied') : t('common.copy')}
                      </Btn>
                    )}
                  </div>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)', whiteSpace: 'pre-wrap' }}>
                    {item.rawTranscript || t('history.rawEmpty')}
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
                {item.appName && <span>{t('history.insertedTo')} <b style={{ color: 'var(--ol-ink-2)' }}>{item.appName}</b></span>}
                <span>{t('history.chars', { count: item.finalText.length })}</span>
                {item.dictionaryEntryCount != null && item.dictionaryEntryCount > 0 && (
                  <span>{t('history.vocabHits', { count: item.dictionaryEntryCount })}</span>
                )}
                <span>{
                  item.insertStatus === 'inserted'
                    ? t('history.inserted')
                    : item.insertStatus === 'pasteSent'
                      ? t('history.pasteSent')
                    : item.insertStatus === 'copiedFallback'
                      ? t('history.copiedFallback', { shortcut: os === 'mac' ? '⌘V' : 'Ctrl+V' })
                      : t('history.insertFailed')
                }</span>
              </div>
            </>
          ) : (
            <div style={{ padding: 40, textAlign: 'center', fontSize: 13, color: 'var(--ol-ink-4)' }}>
              {loading ? t('common.loading') : loadError ? t('history.loadFailed', { err: loadError }) : t('history.selectHint')}
            </div>
          )}
        </Card>
        )}
      </div>
    </div>
  );
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** 当 session.hasAudioRecording 为 true 时渲染：一个加载按钮 + 拿到字节后切换为
 *  原生 audio controls。Blob URL 在组件 unmount 时 revoke，避免泄漏。
 *  `onMissing` 在后端返回 'recording not found'（wav 已被 prune）时触发，让父组件
 *  把按钮永久隐藏，避免用户继续点击得到同样错误。 */
function AudioRecordingPlayer({
  sessionId,
  onMissing,
}: {
  sessionId: string;
  onMissing?: () => void;
}) {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [errorText, setErrorText] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      if (url) URL.revokeObjectURL(url);
    };
  }, [url]);

  const load = async () => {
    setStatus('loading');
    setErrorText(null);
    try {
      const bytes = await readAudioRecording(sessionId);
      if (bytes.byteLength === 0) throw new Error('empty recording');
      // typed array 在严格 TS lib 下不直接是 BlobPart；构造独立 ArrayBuffer 后 cast。
      const buffer = new ArrayBuffer(bytes.byteLength);
      new Uint8Array(buffer).set(bytes);
      const blob = new Blob([buffer], { type: 'audio/wav' });
      const objectUrl = URL.createObjectURL(blob);
      setUrl(objectUrl);
      setStatus('ready');
    } catch (error) {
      console.error('[history] load recording failed', error);
      const msg = errorMessage(error);
      // 文件被清理：通知父组件隐藏按钮组，自身不显示 error UI（用户没干错事）。
      if (msg.includes('recording not found') || msg.includes('not found')) {
        onMissing?.();
        return;
      }
      setStatus('error');
      setErrorText(msg);
    }
  };

  if (status === 'ready' && url) {
    return (
      <div style={{ marginBottom: 14 }}>
        <audio src={url} controls preload="auto" autoPlay style={{ width: '100%' }} />
      </div>
    );
  }
  return (
    <div style={{ marginBottom: 14, display: 'flex', alignItems: 'center', gap: 10 }}>
      <Btn
        icon="play"
        variant="ghost"
        size="sm"
        onClick={() => void load()}
        disabled={status === 'loading'}
      >
        {status === 'loading' ? t('history.audioLoading') : t('history.playRecording')}
      </Btn>
      {status === 'error' && (
        <span style={{ fontSize: 11, color: 'var(--ol-err)' }}>{errorText}</span>
      )}
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

function formatDuration(ms: number | null, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  return t('common.durationMinutes', { value: (sec / 60).toFixed(1) });
}
