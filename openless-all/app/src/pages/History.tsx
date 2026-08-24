// History.tsx — 接 Tauri 后端 list_history / delete_history_entry / clear_history。
// 真实数据来自 ~/Library/Application Support/OpenLess/history.json。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { Tooltip } from '../components/Tooltip';
import { detectOS } from '../components/WindowChrome';
import { formatComboLabel } from '../lib/hotkey';
import { clearHistory, deleteHistoryEntry, listHistory, listStylePacks, readAudioRecording, repolish, resumeCancelledRecording, retranscribeRecording, isTauri } from '../lib/ipc';
import { defaultPackId, packDisplayName, resolveRepolishRetryPackIdWithFallback } from '../lib/history-repolish';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { DictationSession, PolishMode, StylePack } from '../lib/types';
import { countCodePoints } from '../lib/unicode';
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

// Pill 默认 nowrap + flexShrink: 0，遇上长包名会把同一排的按钮挤变形（「复制」文字竖排）。
// 显示包名的地方一律改成可收缩 + 省略号，全名挂在外层容器的 title 上悬停查看。
const TRUNCATED_PILL_STYLE = {
  minWidth: 0,
  maxWidth: '100%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  display: 'block',
  flexShrink: 1,
} as const;

// 历史条目上显示「哪个风格包产出的这段文本」。session.mode 只是风格包的 baseMode
// （四个内置分类之一），自建包全都会落进这四个桶，光看 mode 分不出是哪个包——
// 所以优先用 stylePackId 查真实包名，跟本页「重新润色」面板里的风格命名对齐。
// 内置包例外与命名规则统一走 packDisplayName；旧历史没有 stylePackId、或包已被删除
// 时同样回落到 mode 名。
function styleLabelFor(
  session: DictationSession,
  allPacks: StylePack[] | null,
  modeLabel: Record<PolishMode, string>,
): string {
  const pack = session.stylePackId
    ? allPacks?.find(candidate => candidate.id === session.stylePackId)
    : undefined;
  return pack ? packDisplayName(pack, modeLabel) : modeLabel[session.mode];
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
  const [resumingRecording, setResumingRecording] = useState(false);
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
  // 风格包在本页有两个用途：给历史条目显示包名、给「重新润色」面板选风格。加载提到这里
  // 一次拿全，两处共用，省掉切换条目时 RepolishPanel 重挂载带来的重复 IPC。
  // 注意这里存的是**全部**包（含已禁用）：历史条目可能出自后来被禁用的包，显示名字要能查到；
  // RepolishPanel 自己再 filter(enabled)，禁用的包不该出现在可选风格里。
  const [allPacks, setAllPacks] = useState<StylePack[] | null>(null);
  const [packsError, setPacksError] = useState<string | null>(null);

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

  useEffect(() => {
    let cancelled = false;
    listStylePacks()
      .then(packs => {
        if (!cancelled) setAllPacks(packs);
      })
      .catch(err => {
        if (cancelled) return;
        console.error('[history] failed to load style packs', err);
        setPacksError(errorMessage(err));
      });
    return () => { cancelled = true; };
  }, []);

  // 不缓存：MODE_LABEL 每次渲染都是新对象，用 useCallback 反而会把旧语言的标签闭包
  // 留在缓存里，切换界面语言后 Pill 不跟着变。只在渲染里调用，重建成本可忽略。
  const styleLabel = (session: DictationSession) => styleLabelFor(session, allPacks, MODE_LABEL);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const searchShortcut = os === 'mac' ? '⌘K' : 'Ctrl+K';

  // 搜索词防抖：随输入实时更新 query，300ms 后落到 debouncedQuery 再过滤，
  // 避免每个按键都重算整张列表（与 Marketplace 同模式）。
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(id);
  }, [query]);

  // ⌘K / Ctrl+K 聚焦搜索框（设计稿提示的快捷键）；⌘R / Ctrl+R 刷新历史列表
  // （与浏览器「重新加载」直觉一致）。preventDefault 拦掉 webview 默认的整页
  // reload，改为只重拉 listHistory，避免整个前端重挂载。
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        searchInputRef.current?.focus();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && (e.key === 'r' || e.key === 'R')) {
        e.preventDefault();
        void refresh();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [refresh]);

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
      // Wry/WebKit 中 data URL 的 <a download> 可能不触发保存对话框，后端直接调系统对话框
      if (isTauri) {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('export_audio_recording', { sessionId: item.id });
      } else {
        const dataUrl = await readAudioRecording(item.id);
        if (!dataUrl || dataUrl === 'data:audio/wav;base64,') throw new Error('empty recording');
        const a = document.createElement('a');
        a.href = dataUrl;
        a.download = `openless-recording-${item.id}.wav`;
        document.body.appendChild(a);
        a.click();
        document.body.removeChild(a);
      }
      setActionError(null);
    } catch (error) {
      console.error('[history] failed to export recording', error);
      const msg = errorMessage(error);
      if (isUserCancelled(msg)) {
        setActionError(null);
        return;
      }
      if (msg === 'recording export failed') {
        setActionError(t('history.exportError'));
        return;
      }
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
    if (!item || !item.hasAudioRecording || resumingRecording) return;
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

  const onResumeRecording = async () => {
    if (!item || item.errorCode !== 'recordingCancelled' || retranscribing) return;
    const resumedId = item.id;
    setResumingRecording(true);
    setActionError(null);
    try {
      await resumeCancelledRecording(resumedId);
      setItems(prev => prev.filter(session => session.id !== resumedId));
      setSelectedId(null);
      if (mobile) setMobileDetailOpen(false);
    } catch (error) {
      console.error('[history] resume recording failed', error);
      const msg = errorMessage(error);
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(resumedId);
      } else {
        setActionError(t('history.continueRecordingFailed', { err: msg }));
      }
    } finally {
      setResumingRecording(false);
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
                  {s.errorCode === 'recordingCancelled'
                    ? t('history.cancelledRecording')
                    : s.finalText.split('\n')[0]}
                </div>
                {/* tone 仍按 baseMode 走：颜色保留原来的粗分类信息，文字换成实际风格包名。 */}
                <div style={{ display: 'flex', minWidth: 0 }} title={styleLabel(s)}>
                  <Pill size="sm" tone={s.mode === 'raw' ? 'outline' : 'default'} style={TRUNCATED_PILL_STYLE}>
                    {styleLabel(s)}
                  </Pill>
                </div>
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
                <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
                  <span style={{ fontSize: 13, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)', flexShrink: 0 }}>{formatTime(item.createdAt)}</span>
                  <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
                    <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>{styleLabel(item)}</Pill>
                  </span>
                  {item.pipelineMode === 'multimodal' && (
                    <Pill size="sm" tone="blue">{t('history.multimodalPipeline')}</Pill>
                  )}
                  {/* 「录音」前缀：与下方识别/润色耗时区分——录音时长发生在松键前，
                      不该与流水线各步耗时加总（用户反馈"时间对不上"）。 */}
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{t('history.recorded', { duration: formatDuration(item.durationMs, t) })}</span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  {item.errorCode === 'recordingCancelled'
                    && item.hasAudioRecording
                    && !audioMissingIds.has(item.id) && (
                    <Btn icon="mic" variant="primary" size="sm" disabled={resumingRecording || retranscribing} onClick={() => void onResumeRecording()}>
                      {resumingRecording ? t('history.continuingRecording') : t('history.continueRecording')}
                    </Btn>
                  )}
                  {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                    <Btn icon="download" variant="ghost" size="sm" disabled={resumingRecording || retranscribing} onClick={() => void onExportAudio()}>{t('history.exportRecording')}</Btn>
                  )}
                  {item.hasAudioRecording
                    && !audioMissingIds.has(item.id)
                    && item.pipelineMode !== 'multimodal'
                    && (item.errorCode === 'transcribeFailed' || item.errorCode === 'emptyTranscript' || item.errorCode === 'recordingCancelled') && (
                    <Btn icon="refresh" variant="ghost" size="sm" disabled={retranscribing || resumingRecording} onClick={() => void onRetranscribe()}>
                      {retranscribing
                        ? t('history.retranscribing')
                        : item.errorCode === 'recordingCancelled'
                          ? t('history.startTranscription')
                          : t('history.retranscribe')}
                    </Btn>
                  )}
                  <Btn icon="trash" variant="ghost" size="sm" disabled={resumingRecording || retranscribing} onClick={onDelete}>{t('common.delete')}</Btn>
                </div>
              </div>
              {item.errorCode === 'recordingCancelled' && (
                <div style={{ marginBottom: 14, padding: '9px 11px', borderRadius: 9, background: 'var(--ol-blue-soft)', color: 'var(--ol-ink-2)', fontSize: 12, lineHeight: 1.55 }}>
                  {t('history.cancelledRecordingHint')}
                </div>
              )}
              {/* key 必须带组件前缀：下面的 RepolishPanel 是同一层的兄弟节点，两个都写
                  裸 `item.id` 会让同层出现重复 key，React 只警告不报错，但 reconcile 匹配
                  不上旧 fiber —— 每切换一次历史条目就在 DOM 里残留一个「播放录音」按钮，
                  开着不关的窗口能叠出一整列。 */}
              {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                <AudioRecordingPlayer
                  sessionId={item.id}
                  onMissing={() => markAudioMissing(item.id)}
                  key={`audio-${item.id}`}
                />
              )}
              {/* 流水线明细：识别 / 润色 / 插入 三步各占一行 —— 左列步骤名、中列
                  provider·model（或插入目标），右列该步耗时/状态。旧历史没有模型与
                  耗时字段时对应行自动隐藏，只剩插入行 = 改版前的信息量。 */}
              <div style={{ marginBottom: 16, paddingBottom: 14, borderBottom: '0.5px solid var(--ol-line-soft)', display: 'grid', gridTemplateColumns: 'auto 1fr auto', columnGap: 14, rowGap: 7, fontSize: 11, color: 'var(--ol-ink-4)', alignItems: 'baseline' }}>
                {(item.asrProvider || item.asrMs != null) && (
                  <>
                    <span style={{ display: 'flex' }}>
                      <Tooltip content={t('history.stepAsrHint')} wrap placement="bottom" focusable>
                        <span style={{ cursor: 'help', textDecoration: 'underline dotted', textDecorationColor: 'var(--ol-ink-4)', textUnderlineOffset: 3 }}>
                          {t('history.stepAsr')}
                        </span>
                      </Tooltip>
                    </span>
                    <span style={{ color: 'var(--ol-ink-2)', fontFamily: 'var(--ol-font-mono)', overflowWrap: 'anywhere' }}>
                      {[item.asrProvider, item.asrModel].filter(Boolean).join(' · ')}
                    </span>
                    <span style={{ fontFamily: 'var(--ol-font-mono)', textAlign: 'right', whiteSpace: 'nowrap' }}>
                      {item.asrMs != null ? formatStepDuration(item.asrMs, t) : ''}
                    </span>
                  </>
                )}
                {item.asrDictionaryDelivery && (
                  <>
                    <span>{t('history.stepAsrDictionary', { defaultValue: 'ASR vocabulary' })}</span>
                    <span style={{ color: 'var(--ol-ink-2)', fontFamily: 'var(--ol-font-mono)', overflowWrap: 'anywhere' }}>
                      {item.asrDictionaryDelivery.mode}
                      {item.asrDictionaryDelivery.reason ? ` · ${item.asrDictionaryDelivery.reason}` : ''}
                    </span>
                    <span style={{ fontFamily: 'var(--ol-font-mono)', textAlign: 'right', whiteSpace: 'nowrap' }}>
                      {t('history.dictionaryDeliveryCounts', { defaultValue: '{{sent}} sent / {{dropped}} omitted', sent: item.asrDictionaryDelivery.sentCount, dropped: item.asrDictionaryDelivery.droppedCount })}
                    </span>
                  </>
                )}
                {(item.llmProvider || item.llmModel || item.polishMs != null) && (
                  <>
                    <span>{t('history.stepPolish')}</span>
                    <span style={{ color: 'var(--ol-ink-2)', fontFamily: 'var(--ol-font-mono)', overflowWrap: 'anywhere' }}>
                      {[item.llmProvider, item.llmModel].filter(Boolean).join(' · ')}
                    </span>
                    <span style={{ fontFamily: 'var(--ol-font-mono)', textAlign: 'right', whiteSpace: 'nowrap' }}>
                      {item.polishMs != null ? formatStepDuration(item.polishMs, t) : ''}
                    </span>
                  </>
                )}
                {(item.llmDictionaryDelivery || item.llmDictionarySentCount != null) && (
                  <>
                    <span>{t('history.stepLlmDictionary', { defaultValue: 'LLM vocabulary' })}</span>
                    <span style={{ color: 'var(--ol-ink-2)' }}>
                      {item.llmDictionaryDelivery?.mode ?? t('history.dictionaryPrompt', { defaultValue: 'Included in the polish prompt' })}
                      {item.llmDictionaryDelivery?.reason ? ` · ${item.llmDictionaryDelivery.reason}` : ''}
                    </span>
                    <span style={{ fontFamily: 'var(--ol-font-mono)', textAlign: 'right', whiteSpace: 'nowrap' }}>
                      {item.llmDictionaryDelivery
                        ? t('history.dictionaryDeliveryCounts', { defaultValue: '{{sent}} sent / {{dropped}} omitted', sent: item.llmDictionaryDelivery.sentCount, dropped: item.llmDictionaryDelivery.droppedCount })
                        : item.llmDictionarySentCount}
                    </span>
                  </>
                )}
                <span>{t('history.stepInsert')}</span>
                <span style={{ color: 'var(--ol-ink-2)' }}>
                  {item.appName && <><b>{item.appName}</b>{' · '}</>}
                  {/* 按 Unicode 码点计（emoji / CJK 扩展 B 等增补平面字符不按 UTF-16 码元双算），
                      与后端 `polished.chars().count()` 及概览页「字数」口径一致。 */}
                  {t('history.chars', { count: countCodePoints(item.finalText) })}
                  {item.dictionaryEntryCount != null && item.dictionaryEntryCount > 0 && (
                    <>{' · '}{t('history.vocabHits', { count: item.dictionaryEntryCount })}</>
                  )}
                </span>
                <span style={{ textAlign: 'right', whiteSpace: 'nowrap' }}>{
                  item.insertStatus === 'inserted'
                    ? t('history.inserted')
                    : item.insertStatus === 'pasteSent'
                      ? t('history.pasteSent')
                    : item.insertStatus === 'copiedFallback'
                      ? t('history.copiedFallback', { shortcut: os === 'mac' ? '⌘V' : 'Ctrl+V' })
                      : item.errorCode === 'recordingCancelled'
                        ? t('history.pendingTranscription')
                        : item.errorCode == null && item.hasAudioRecording && item.rawTranscript
                          ? t('history.transcribedOnly')
                          : t('history.insertFailed')
                }</span>
              </div>
              {/* minWidth: 0 —— grid 子项默认 min-width: auto，任何不换行的内容（这里是风格包名
                  Pill）都会把整列撑出卡片、逼出横向滚动条。两栏都要加，否则一栏撑宽另一栏跟着宽。 */}
              <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12 }}>
                <div style={{ minWidth: 0, padding: 14, border: '0.5px solid var(--ol-line)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
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
                <div style={{ minWidth: 0, padding: 14, border: '0.5px solid var(--ol-blue)', borderRadius: 10, background: 'var(--ol-blue-soft)' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 10 }}>
                    <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
                      <Pill size="sm" tone="blue" style={TRUNCATED_PILL_STYLE}>{styleLabel(item)}</Pill>
                    </span>
                    {/* 「复制」不能被长包名压缩：压窄后按钮文字会竖排。 */}
                    <span style={{ flexShrink: 0 }}>
                      <Btn icon={justCopied ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopy()}>
                        {justCopied ? t('common.copied') : t('common.copy')}
                      </Btn>
                    </span>
                  </div>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)', whiteSpace: 'pre-line' }}>
                    {item.finalText}
                  </p>
                </div>
              </div>
              {/* 重新润色：拿这条的原文再跑一次 LLM。没有原文就没得润色（转录失败条目），
                  此时整块不渲染；QA 记录的原文是问题而不是待润色文本，同样不渲染。
                  key 让切换记录时结果与状态一起重置，避免把上一条的结果留在新条目下面；
                  前缀是为了跟上面播放器的 key 区分开（同层重复 key 会残留旧节点）。 */}
              {item.rawTranscript.trim() && item.errorCode !== 'qaSession' && (
                <RepolishPanel
                  session={item}
                  mobile={mobile}
                  allPacks={allPacks}
                  packsError={packsError}
                  key={`repolish-${item.id}`}
                />
              )}
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

/** 后端超时错误在 IPC 边界退化成裸字符串（LLMError::Timeout → "timeout"）。
 *  只匹配整串的常见超时形态，避免其它含 "timeout" 字样的错误被误判成超时。 */
function isTimeout(message: string): boolean {
  const trimmed = message.trim();
  return /^(timeout|timed out|request timed out)$/i.test(trimmed) || trimmed.includes('超时');
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

interface RepolishResult {
  /** 结果卡片的 key。同一个风格重复应用会覆盖上一次，不无限堆卡片。 */
  key: string;
  title: string;
  text: string;
}

/**
 * 「重新润色」面板：拿这条历史的**原文**再跑一次 LLM。
 *
 * 两个入口共用一条后端通道（`repolish`，stylePackId 可选）：
 * - 「用原风格重试」→ 优先传产生这条记录的风格包 id（包已删除/旧历史/未加载时回落当前
 *   激活风格）。用同一套风格再跑一遍，才能判断上次的结果是模型抖动还是稳定行为 ——
 *   这是用户说「AI 识别得不对」时真正想做的对照实验。
 * - 「应用」→ 传选中的 pack id，看同一段话换个风格是什么样。
 *
 * 结果只在本次查看时显示，不写回历史条目：历史的 finalText 是「当时真的插进去的那段
 * 文字」，是一条事实记录，不该被事后试算覆盖。面板顶部的说明也把这点直说了。
 *
 * 注意这里只重跑润色，不重跑识别 —— 成功听写的录音在插入后就删了（隐私设计），
 * 原文是唯一还在的输入。真正的「重新转录」入口仍只对留有录音的失败条目开放。
 */
function RepolishPanel({ session, mobile, allPacks, packsError }: {
  session: DictationSession;
  mobile: boolean;
  /** History 顶层加载的**全部**风格包（含已禁用）；null 表示还在加载。 */
  allPacks: StylePack[] | null;
  packsError: string | null;
}) {
  const { t } = useTranslation();
  const MODE_LABEL = useModeLabel();
  const [selectedPackId, setSelectedPackId] = useState<string>('');
  const [running, setRunning] = useState<'retry' | 'apply' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [results, setResults] = useState<RepolishResult[]>([]);

  // 只列启用的包：禁用的包在别处也不参与润色，这里列出来会让「应用」得到
  // 一个用户以为已经关掉的风格。
  const packs = useMemo(
    () => (allPacks ? allPacks.filter(p => p.enabled) : null),
    [allPacks],
  );

  useEffect(() => {
    if (!packs) return;
    setSelectedPackId(current => current || defaultPackId(packs));
  }, [packs]);

  const run = async (kind: 'retry' | 'apply') => {
    // 重试优先用产生这条记录的原包；原包已删除/旧历史/未加载时显式落到当前激活包
    // （其次第一个可用包）——前端标注与实际执行一致，而不是让后端走 None 兜底链。
    const packId = kind === 'apply'
      ? selectedPackId
      : resolveRepolishRetryPackIdWithFallback(session, allPacks, packs ?? []);
    if (kind === 'apply' && !packId) return;
    setRunning(kind);
    setError(null);
    try {
      const text = await repolish(session.rawTranscript, session.mode, packId);
      // 用 allPacks 而非 packs 找包名：按已禁用原包重试时标题仍显示真实包名。
      const pack = packId ? allPacks?.find(p => p.id === packId) : undefined;
      const result: RepolishResult = {
        key: packId ?? '__retry__',
        title: pack
          ? t('history.repolish.resultTitle', { name: packDisplayName(pack, MODE_LABEL) })
          : t('history.repolish.retryResultTitle'),
        text,
      };
      // 同一个 key 覆盖旧结果，新 key 追加到最前面 —— 最新的试算结果离操作区最近。
      setResults(prev => [result, ...prev.filter(r => r.key !== result.key)]);
    } catch (err) {
      console.error('[history] repolish failed', err);
      const msg = errorMessage(err);
      // 后端把 LLMError::Timeout 原样透成字符串 "timeout"，直接显示等于没说 ——
      // 用户看到「重新润色失败：timeout」只会以为是这个功能坏了，而实际是当前 LLM
      // provider 没在 30 秒内回包（免费模型池尤其常见）。换一句能照着做的提示。
      setError(
        isTimeout(msg)
          ? t('history.repolish.timeout')
          : t('history.repolish.failed', { err: msg }),
      );
    } finally {
      setRunning(null);
    }
  };

  return (
    <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div style={{ display: 'flex', alignItems: 'baseline', justifyContent: 'space-between', gap: 10, flexWrap: 'wrap', marginBottom: 4 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
          {t('history.repolish.title')}
        </span>
        {results.length > 0 && (
          <Btn size="sm" variant="ghost" onClick={() => setResults([])}>
            {t('history.repolish.clear')}
          </Btn>
        )}
      </div>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', lineHeight: 1.55, marginBottom: 12 }}>
        {t('history.repolish.hint')}
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', marginBottom: results.length > 0 ? 14 : 0 }}>
        <Btn
          icon="refresh"
          variant="ghost"
          size="sm"
          disabled={running !== null}
          onClick={() => void run('retry')}
        >
          {running === 'retry' ? t('history.repolish.retrying') : t('history.repolish.retry')}
        </Btn>
        <span style={{ width: 1, height: 18, background: 'var(--ol-line)' }} />
        {packsError ? (
          <span style={{ fontSize: 11, color: 'var(--ol-red, #ef4444)' }}>
            {t('history.repolish.packsLoadFailed', { err: packsError })}
          </span>
        ) : packs && packs.length === 0 ? (
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{t('history.repolish.noPacks')}</span>
        ) : (
          <>
            <select
              value={selectedPackId}
              onChange={e => setSelectedPackId(e.target.value)}
              aria-label={t('history.repolish.pickStyle')}
              disabled={!packs || running !== null}
              style={{
                padding: '4px 8px',
                fontSize: 11.5,
                fontFamily: 'inherit',
                color: 'var(--ol-ink-2)',
                background: 'var(--ol-surface-2)',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8,
                maxWidth: mobile ? 160 : 220,
              }}
            >
              {(packs ?? []).map(pack => (
                <option key={pack.id} value={pack.id}>{packDisplayName(pack, MODE_LABEL)}</option>
              ))}
            </select>
            <Btn
              variant="ghost"
              size="sm"
              disabled={!selectedPackId || running !== null}
              onClick={() => void run('apply')}
            >
              {running === 'apply' ? t('history.repolish.applying') : t('history.repolish.apply')}
            </Btn>
          </>
        )}
      </div>

      {error && (
        <div style={{ marginTop: 10, padding: '8px 10px', borderRadius: 8, background: 'rgba(239,68,68,0.08)', color: 'var(--ol-red, #ef4444)', fontSize: 11.5, lineHeight: 1.45 }}>
          {error}
        </div>
      )}

      {results.length > 0 && (
        <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12 }}>
          {results.map(result => (
            <RepolishResultCard key={result.key} title={result.title} text={result.text} />
          ))}
        </div>
      )}
    </div>
  );
}

function RepolishResultCard({ title, text }: { title: string; text: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy repolish result', error);
    }
  };

  return (
    // minWidth: 0 —— grid 子项默认 min-width: auto，标题 Pill 不换行时会把卡片
    // 撑出结果网格（与详情页两栏文本卡片同一类问题）。
    <div style={{ minWidth: 0, padding: 14, border: '0.5px dashed var(--ol-line-strong)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8, marginBottom: 10 }}>
        <span style={{ display: 'flex', minWidth: 0 }} title={title}>
          <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>{title}</Pill>
        </span>
        {text.trim() && (
          <Btn icon={copied ? 'check' : 'copy'} variant="ghost" size="sm" onClick={() => void onCopy()}>
            {copied ? t('common.copied') : t('common.copy')}
          </Btn>
        )}
      </div>
      <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)', whiteSpace: 'pre-wrap' }}>
        {text.trim() || t('history.repolish.empty')}
      </p>
    </div>
  );
}

function isUserCancelled(message: string): boolean {
  const normalized = message.trim().toLowerCase();
  return normalized === 'cancelled'
    || normalized === 'canceled'
    || normalized === 'user cancelled'
    || normalized === 'user canceled';
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
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [errorText, setErrorText] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const blobUrlRef = useRef<string | null>(null);

  // 组件 unmount 时释放 Blob URL，避免内存泄漏。
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
        blobUrlRef.current = null;
      }
    };
  }, []);

  const clearBlobUrl = () => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
    setBlobUrl(null);
  };

  const load = async () => {
    setStatus('loading');
    setErrorText(null);
    try {
      const dataUrl = await readAudioRecording(sessionId);
      if (!mountedRef.current) return;
      if (!dataUrl || dataUrl === 'data:audio/wav;base64,') throw new Error('empty recording');
      // WebKitGTK <audio> 对 data: URL 解码不稳定（时长 0 / 播不动），
      // 把 base64 解码为二进制再封装成 Blob URL，在 WebKit 里远更可靠。
      const comma = dataUrl.indexOf(',');
      const b64 = comma >= 0 ? dataUrl.slice(comma + 1) : '';
      if (!b64) throw new Error('empty recording');
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const blob = new Blob([bin], { type: 'audio/wav' });
      const url = URL.createObjectURL(blob);
      if (!mountedRef.current) {
        URL.revokeObjectURL(url);
        return;
      }
      if (blobUrlRef.current) URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = url;
      setBlobUrl(url);
      setStatus('ready');
    } catch (error) {
      if (!mountedRef.current) return;
      console.error('[history] load recording failed', error);
      const msg = errorMessage(error);
      if (msg.includes('recording not found') || msg.includes('not found')) {
        onMissing?.();
        return;
      }
      setStatus('error');
      setErrorText(msg);
    }
  };

  if (status === 'ready' && blobUrl) {
    return (
      <div style={{ marginBottom: 14 }}>
        <audio
          src={blobUrl}
          controls
          preload="auto"
          autoPlay
          style={{ width: '100%' }}
          onError={(e) => {
            if (!mountedRef.current) return;
            const a = e.currentTarget;
            const code = a.error?.code ?? -1;
            const detail = a.error?.message ?? `${code}`;
            console.error('[history] <audio> decode/play failed', { code, detail });
            clearBlobUrl();
            setStatus('error');
            setErrorText(t('history.audioDecodeFailed', { err: detail }));
          }}
        />
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

/** 流水线单步耗时：<1s 显示整数毫秒（流式收尾常在几十 ms，0.1s 精度会把不同结果
 *  拍成同一个值，模型对比就失真了——PR #826 review）；≥1s 沿用 0.1s 精度。 */
function formatStepDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms < 1000) return t('common.durationMillis', { value: Math.round(ms) });
  return formatDuration(ms, t);
}

function formatDuration(ms: number | null, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  return t('common.durationMinutes', { value: (sec / 60).toFixed(1) });
}
