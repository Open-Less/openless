// History.tsx — backed by the Tauri commands list_history / delete_history_entry / clear_history.
// Real data lives in ~/Library/Application Support/OpenLess/history.json.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { LearnVocabulary } from '../components/LearnVocabulary';
import { Tooltip } from '../components/Tooltip';
import { AssistantMarkdown } from '../components/chat/markdown';
import { detectOS } from '../components/WindowChrome';
import { formatComboLabel } from '../lib/hotkey';
import {
  clearHistory,
  applyQuickNoteRepolish,
  deleteHistoryEntry,
  listHistory,
  listStylePacks,
  readAudioRecording,
  repolish,
  retranscribeRecording,
  isTauri,
} from '../lib/ipc';
import {
  defaultPackId,
  packDisplayName,
  resolveRepolishRetryPackIdWithFallback,
} from '../lib/history-repolish';
import { canRetranscribeHistoryEntry } from '../lib/history-retranscribe';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { DictationSession, PolishMode, StylePack } from '../lib/types';
import { formatHistoryTime, formatLocaleDecimal, formatLocaleNumber } from '../lib/localeFormat';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';
import { SelectLite } from '../components/ui/SelectLite';

function useModeLabel(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

// Pill defaults to nowrap + flexShrink: 0; a long pack name would squeeze the buttons on the same
// row out of shape (the "Copy" text wrapping vertically). Anywhere a pack name is shown, use a
// shrinkable + ellipsis style, with the full name in the outer container's title for hover.
const TRUNCATED_PILL_STYLE = {
  minWidth: 0,
  maxWidth: '100%',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  display: 'block',
  flexShrink: 1,
} as const;

// Which style pack produced this text, shown on the history entry. session.mode is only the pack's
// baseMode (one of four built-in categories) — all custom packs fall into those buckets, so mode
// alone cannot identify the pack. Prefer resolving the real pack name via stylePackId, matching
// the style naming in this page's repolish panel. Built-in pack exceptions and naming go through
// packDisplayName; old history without stylePackId, or a deleted pack, falls back to the mode
// name too.
function styleLabelFor(
  session: DictationSession,
  allPacks: StylePack[] | null,
  modeLabel: Record<PolishMode, string>,
): string {
  const pack = session.stylePackId
    ? allPacks?.find((candidate) => candidate.id === session.stylePackId)
    : undefined;
  return pack ? packDisplayName(pack, modeLabel) : modeLabel[session.mode];
}

export function History({ quickNotesOnly = false }: { quickNotesOnly?: boolean } = {}) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const os = detectOS();
  const MODE_LABEL = useModeLabel();
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [items, setItems] = useState<DictationSession[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [justCopied, setJustCopied] = useState(false);
  const [justCopiedRaw, setJustCopiedRaw] = useState(false);
  const [showRawTranscript, setShowRawTranscript] = useState(false);
  const [repolishOpen, setRepolishOpen] = useState(false);
  // Retranscription in flight: disable the button and show "Transcribing…", preventing repeated
  // clicks from firing multiple ASR runs.
  const [retranscribing, setRetranscribing] = useState(false);
  const [retranscriptionResult, setRetranscriptionResult] = useState<{
    sessionId: string;
    text: string;
  } | null>(null);
  const [playbackRequest, setPlaybackRequest] = useState(0);
  const [audioLoading, setAudioLoading] = useState(false);
  // Lazily-detected missing recording files: after retention / count-cap cleanup the wav may be
  // gone from disk while the history entry still has hasAudioRecording=true. When any component
  // (playback / export) first gets 'recording not found' over IPC, add the id here so the
  // render condition flips to false, avoiding repeated clicks producing the same error.
  // Fixes the pr_agent "Missing file check" feedback.
  const [audioMissingIds, setAudioMissingIds] = useState<Set<string>>(() => new Set());
  const markAudioMissing = useCallback((id: string) => {
    setAudioMissingIds((prev) => {
      if (prev.has(id)) return prev;
      const next = new Set(prev);
      next.add(id);
      return next;
    });
  }, []);
  const { prefs, updatePrefs } = useHotkeySettings();
  // The list/detail split needs space after the main sidebar and page padding.
  const mobile = useMobileLayout(1000);
  const [mobileDetailOpen, setMobileDetailOpen] = useState(() => !mobile);
  // A wide layout already shows the detail. Keep that same subtree mounted
  // when shrinking, including its in-flight or completed repolish result.
  useEffect(() => {
    if (!mobile) setMobileDetailOpen(true);
  }, [mobile]);

  // Style packs have two uses on this page: pack names on history entries and style selection in
  // the repolish panel. Load them once up here so both share the data, avoiding duplicate IPC
  // from RepolishPanel remounts when switching entries. Note this stores **all** packs
  // (including disabled): a history entry may come from a pack disabled later, so its name must
  // stay resolvable; RepolishPanel filters(enabled) itself — disabled packs must not appear among
  // the selectable styles.
  const [allPacks, setAllPacks] = useState<StylePack[] | null>(null);
  const [packsError, setPacksError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listHistory();
      const visible = quickNotesOnly ? data.filter((entry) => entry.source === 'quick_note') : data;
      setItems(visible);
      setActionError(null);
      setSelectedId((prev) =>
        prev && visible.some((s) => s.id === prev) ? prev : (visible[0]?.id ?? null),
      );
    } catch (error) {
      console.error('[history] failed to load history', error);
      setLoadError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, [quickNotesOnly]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    listStylePacks()
      .then((packs) => {
        if (!cancelled) setAllPacks(packs);
      })
      .catch((err) => {
        if (cancelled) return;
        console.error('[history] failed to load style packs', err);
        setPacksError(errorMessage(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Do not memoize: MODE_LABEL is a fresh object each render, and useCallback would cache the
  // old-language label closure, leaving Pills untranslated after a UI language switch. Called
  // only during render; rebuilding is negligible.
  const styleLabel = (session: DictationSession) => styleLabelFor(session, allPacks, MODE_LABEL);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const searchShortcut = os === 'mac' ? '⌘K' : 'Ctrl+K';

  // Debounce the search term: update query live on input, then after 300ms commit to
  // debouncedQuery and filter, avoiding recomputing the whole list on every keystroke (same
  // pattern as Marketplace).
  useEffect(() => {
    const id = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(id);
  }, [query]);

  // ⌘K / Ctrl+K focuses the search box (the shortcut from the design); ⌘R / Ctrl+R refreshes the
  // history list (matching the browser "reload" instinct). preventDefault stops the webview's
  // default full-page reload; only listHistory is re-fetched, so the frontend does not remount.
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
    const q = debouncedQuery.trim().toLowerCase();
    if (!q) return items;
    // Match keywords against both the raw transcript and the polished text, covering both things
    // the user might remember.
    return items.filter(
      (s) => s.rawTranscript.toLowerCase().includes(q) || s.finalText.toLowerCase().includes(q),
    );
  }, [items, debouncedQuery]);
  const item = useMemo(
    () => filtered.find((s) => s.id === selectedId) || filtered[0],
    [filtered, selectedId],
  );

  useEffect(() => {
    setShowRawTranscript(false);
    setRepolishOpen(false);
  }, [item?.id]);
  const handleAudioMissing = useCallback(() => {
    if (item?.id) markAudioMissing(item.id);
  }, [item?.id, markAudioMissing]);

  const onClear = async () => {
    const clearable = items.filter((entry) => entry.source !== 'quick_note');
    if (clearable.length === 0) return;
    if (!confirm(t('history.confirmClear', { count: clearable.length }))) return;
    setActionError(null);
    try {
      await clearHistory();
      setItems((prev) => prev.filter((entry) => entry.source === 'quick_note'));
      setSelectedId((current) => {
        const remaining = items.filter((entry) => entry.source === 'quick_note');
        return current && remaining.some((entry) => entry.id === current)
          ? current
          : (remaining[0]?.id ?? null);
      });
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
      setItems((prev) => prev.filter((s) => s.id !== deletedId));
      setSelectedId((current) => (current === deletedId ? null : current));
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
      // When polish failed / produced nothing, finalText is empty; fall back to the raw text so
      // the Copy button never copies an empty string, leaving the original unreachable from the
      // UI (the recognized text survives even when polish fails).
      await navigator.clipboard.writeText(
        item.finalText.trim() ? item.finalText : item.rawTranscript,
      );
      setActionError(null);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy entry', error);
      setActionError(t('history.copyFailed', { err: errorMessage(error) }));
    }
  };

  // Copy the raw (recognized) text separately: for polish failures or when the user wants the
  // unpolished text.
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
      // In Wry/WebKit an <a download> with a data URL may not open the save dialog; the backend
      // invokes the system dialog directly.
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
      // wav already removed by retention / count-cap cleanup: hide the button and show no error
      // (the user did nothing wrong).
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(item.id);
        return;
      }
      setActionError(t('history.exportFailed', { err: msg }));
    }
  };

  const onShareAudio = async () => {
    if (!item?.hasAudioRecording) return;
    try {
      const dataUrl = await readAudioRecording(item.id);
      const comma = dataUrl.indexOf(',');
      const b64 = comma >= 0 ? dataUrl.slice(comma + 1) : '';
      if (!b64) throw new Error('empty recording');
      const bin = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      const file = new File([bin], `openless-recording-${item.id}.wav`, {
        type: 'audio/wav',
      });
      if (!navigator.share || (navigator.canShare && !navigator.canShare({ files: [file] }))) {
        await onExportAudio();
        return;
      }
      await navigator.share({
        title: historyTitle(item, t),
        files: [file],
      });
    } catch (error) {
      const msg = errorMessage(error);
      if (!isUserCancelled(msg)) {
        setActionError(t('history.exportFailed', { err: msg }));
      }
    }
  };

  const onChooseExportDirectory = async () => {
    if (!quickNotesOnly || !prefs || os === 'android') return;
    try {
      let picked: string | null = null;
      if (isTauri) {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selection = await open({
          directory: true,
          multiple: false,
          title: t('history.chooseSaveDirectory', '选择转写文件保存位置'),
        });
        picked = Array.isArray(selection) ? null : selection;
      } else {
        picked = window.prompt(
          t('history.saveDirectoryPrompt', '输入转写文件保存目录'),
          prefs.quickNoteExportDirectory,
        );
      }
      const directory = picked?.trim();
      if (!directory) return;
      await updatePrefs({ ...prefs, quickNoteExportDirectory: directory });
      setActionError(null);
    } catch (error) {
      console.error('[history] failed to choose export directory', error);
      setActionError(t('history.saveDirectoryFailed', { err: errorMessage(error) }));
    }
  };

  const onResetExportDirectory = async () => {
    if (!prefs || !prefs.quickNoteExportDirectory) return;
    try {
      await updatePrefs({ ...prefs, quickNoteExportDirectory: '' });
      setActionError(null);
    } catch (error) {
      console.error('[history] failed to reset export directory', error);
      setActionError(t('history.saveDirectoryFailed', { err: errorMessage(error) }));
    }
  };

  // Failed entries keep the #613 in-place fix; completed / polish-failed entries that already
  // inserted text only show a temporary result, avoiding passing off a re-transcribed text as the
  // historical fact of what was actually inserted.
  const onRetranscribe = async () => {
    if (!item || !canRetranscribeHistoryEntry(item)) return;
    const sessionId = item.id;
    setRetranscribing(true);
    setRetranscriptionResult(null);
    setActionError(null);
    try {
      const result = await retranscribeRecording(sessionId);
      if (result.updatedEntry) {
        const updatedEntry = result.updatedEntry;
        setItems((prev) =>
          prev.map((entry) => (entry.id === updatedEntry.id ? updatedEntry : entry)),
        );
      } else {
        setRetranscriptionResult({ sessionId, text: result.text });
      }
    } catch (error) {
      console.error('[history] retranscribe failed', error);
      const msg = errorMessage(error);
      // wav already removed by retention / count-cap cleanup: hide the entry point, no error
      // (the user did nothing wrong).
      if (msg.includes('recording not found') || msg.includes('not found')) {
        markAudioMissing(sessionId);
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
        kicker={quickNotesOnly ? t('quickNote.kicker', 'Quick notes') : t('history.kicker')}
        title={quickNotesOnly ? t('quickNote.title', 'Quick notes') : t('history.title')}
        desc={quickNotesOnly ? t('quickNote.desc', 'Permanent audio notes.') : t('history.desc')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>
              {t('common.refresh')}
            </Btn>
            {!quickNotesOnly && (
              <Btn icon="trash" variant="ghost" size="sm" onClick={onClear}>
                {t('common.clear')}
              </Btn>
            )}
          </div>
        }
      />
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: mobile ? '1fr' : '300px 1fr',
          gap: 14,
          flex: 1,
          minHeight: 0,
        }}
      >
        {(!mobile || !mobileDetailOpen) && (
          <Card
            padding={0}
            style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}
          >
            {/* The left column is one scroll container; the search box sticks to the top with an
              opaque background — the list scrolls underneath it. Style filter chips and the
              "N entries" count row are dropped; only search remains. */}
            <div className="ol-thinscroll" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
              <div
                style={{
                  position: 'sticky',
                  top: 0,
                  zIndex: 2,
                  background: 'var(--ol-surface)',
                  padding: '12px 14px',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 6,
                    padding: '6px 10px',
                    fontSize: 12,
                    border: '0.5px solid var(--ol-line-strong)',
                    borderRadius: 8,
                    background: 'var(--ol-surface-2)',
                    color: 'var(--ol-ink-3)',
                  }}
                >
                  <Icon name="search" size={12} />
                  <input
                    ref={searchInputRef}
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder={t('history.searchPlaceholder', { shortcut: searchShortcut })}
                    aria-label={t('history.searchPlaceholder', { shortcut: searchShortcut })}
                    style={{
                      flex: 1,
                      minWidth: 0,
                      outline: 'none',
                      border: 0,
                      background: 'transparent',
                      fontSize: 12,
                      color: 'var(--ol-ink-1)',
                      fontFamily: 'inherit',
                    }}
                  />
                </div>
              </div>
              <div style={{ padding: 6 }}>
                {actionError && (
                  <div
                    style={{
                      margin: 8,
                      padding: '9px 10px',
                      borderRadius: 8,
                      background: 'rgba(239,68,68,0.08)',
                      color: 'var(--ol-red, #ef4444)',
                      fontSize: 12,
                      lineHeight: 1.45,
                    }}
                  >
                    {actionError}
                  </div>
                )}
                {loading && (
                  <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
                    {t('common.loading')}
                  </div>
                )}
                {!loading && loadError && (
                  <div
                    style={{
                      padding: 16,
                      fontSize: 12,
                      color: 'var(--ol-ink-4)',
                      display: 'flex',
                      flexDirection: 'column',
                      alignItems: 'flex-start',
                      gap: 10,
                    }}
                  >
                    <span>{t('history.loadFailed', { err: loadError })}</span>
                    <Btn size="sm" variant="ghost" onClick={() => void refresh()}>
                      {t('history.retry')}
                    </Btn>
                  </div>
                )}
                {!loading && !loadError && filtered.length === 0 && (
                  <div style={{ padding: 16, fontSize: 12, color: 'var(--ol-ink-4)' }}>
                    {debouncedQuery.trim()
                      ? t('history.searchNoMatch', { query: debouncedQuery.trim() })
                      : t('history.empty', {
                          trigger: prefs
                            ? formatComboLabel(
                                (quickNotesOnly
                                  ? prefs.quickNoteHotkey
                                  : prefs.dictationHotkey) ?? {
                                  primary: '',
                                  modifiers: [],
                                },
                              )
                            : '',
                        })}
                  </div>
                )}
                {!loadError &&
                  filtered.map((s) => (
                    <button
                      key={s.id}
                      onClick={() => {
                        setSelectedId(s.id);
                        setPlaybackRequest(0);
                        if (mobile) setMobileDetailOpen(true);
                      }}
                      // Selected items no longer use a blue left bar + pale blue fill — same
                      // neutral language as channel rows: rounded + gray fill + thin outline.
                      style={{
                        width: '100%',
                        padding: '10px 12px',
                        textAlign: 'left',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: 4,
                        border: '0.5px solid',
                        borderColor: selectedId === s.id ? 'var(--ol-line)' : 'transparent',
                        borderRadius: 10,
                        background: selectedId === s.id ? 'var(--ol-surface-2)' : 'transparent',
                        cursor: 'default',
                        fontFamily: 'inherit',
                        marginBottom: 4,
                        transition:
                          'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
                        // Rows fade in and drop slightly when search filters or new records
                        // insert.
                        animation: 'ol-item-in 0.22s var(--ol-motion-spring) both',
                      }}
                    >
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 8,
                        }}
                      >
                        <span
                          style={{
                            fontSize: 11,
                            fontFamily: 'var(--ol-font-mono)',
                            color: 'var(--ol-ink-3)',
                          }}
                        >
                          {formatHistoryTime(s.createdAt, locale)}
                        </span>
                        <span
                          style={{
                            fontSize: 10,
                            color: 'var(--ol-ink-4)',
                            fontFamily: 'var(--ol-font-mono)',
                          }}
                        >
                          {formatDuration(s.durationMs, t, locale)}
                        </span>
                      </div>
                      <div
                        style={{
                          fontSize: 12,
                          color: 'var(--ol-ink-2)',
                          lineHeight: 1.45,
                          display: '-webkit-box',
                          WebkitLineClamp: 2,
                          WebkitBoxOrient: 'vertical',
                          overflow: 'hidden',
                        }}
                      >
                        {historyTitle(s, t)}
                      </div>
                      {/* tone still follows baseMode: the color keeps the original coarse
                        category info, the text becomes the actual style pack name. */}
                      <div style={{ display: 'flex', minWidth: 0 }} title={styleLabel(s)}>
                        <Pill
                          size="sm"
                          tone={s.mode === 'raw' ? 'outline' : 'default'}
                          style={TRUNCATED_PILL_STYLE}
                        >
                          {styleLabel(s)}
                        </Pill>
                      </div>
                    </button>
                  ))}
              </div>
            </div>
          </Card>
        )}

        {(!mobile || mobileDetailOpen) && (
          <Card padding={20} className="ol-thinscroll" style={{ overflow: 'auto' }}>
            {item ? (
              <>
                {mobile && (
                  <div style={{ marginBottom: 12 }}>
                    <Btn
                      icon="chevLeft"
                      variant="ghost"
                      size="sm"
                      onClick={() => setMobileDetailOpen(false)}
                    >
                      {t('history.backToList')}
                    </Btn>
                  </div>
                )}
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    marginBottom: 14,
                    flexWrap: 'wrap',
                    gap: 8,
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
                    <span
                      style={{
                        fontSize: 13,
                        fontFamily: 'var(--ol-font-mono)',
                        color: 'var(--ol-ink-3)',
                        flexShrink: 0,
                      }}
                    >
                      {formatHistoryTime(item.createdAt, locale)}
                    </span>
                    <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
                      <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>
                        {styleLabel(item)}
                      </Pill>
                    </span>
                    {item.pipelineMode === 'multimodal' && (
                      <Pill size="sm" tone="blue">
                        {t('history.multimodalPipeline')}
                      </Pill>
                    )}
                    {/* "Recorded" prefix: distinct from the recognize/polish timings below —
                      recording time happens before the key release and must not be summed with
                      the pipeline steps (user feedback: "times don't add up"). */}
                    <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
                      {t('history.recorded', {
                        duration: formatDuration(item.durationMs, t, locale),
                      })}
                    </span>
                  </div>
                  <HistoryActionMenu
                    hasAudioRecording={
                      item.hasAudioRecording === true && !audioMissingIds.has(item.id)
                    }
                    audioLoading={audioLoading}
                    showShare={os === 'android'}
                    canRetranscribe={
                      canRetranscribeHistoryEntry(item) && !audioMissingIds.has(item.id)
                    }
                    retranscribing={retranscribing}
                    canRepolish={Boolean(item.rawTranscript.trim() || quickNotesOnly)}
                    showSaveDirectory={quickNotesOnly && os !== 'android'}
                    exportDirectory={prefs?.quickNoteExportDirectory ?? ''}
                    onPlay={() => setPlaybackRequest((request) => request + 1)}
                    onExport={() => void onExportAudio()}
                    onShare={() => void onShareAudio()}
                    onRetranscribe={() => void onRetranscribe()}
                    onDelete={onDelete}
                    onRepolish={() => setRepolishOpen(true)}
                    onChooseSaveDirectory={() => void onChooseExportDirectory()}
                    onResetSaveDirectory={() => void onResetExportDirectory()}
                  />
                </div>
                {/* The key must carry a component prefix: RepolishPanel below is a sibling at the
                  same level; if both used bare `item.id` the level would have duplicate keys.
                  React only warns (no error), but reconcile fails to match the old fiber — every
                  entry switch leaves a stale playback control in the DOM, and a long-open window
                  can stack up a whole column of them. */}
                {item.hasAudioRecording && !audioMissingIds.has(item.id) && (
                  <AudioRecordingPlayer
                    sessionId={item.id}
                    onMissing={handleAudioMissing}
                    playRequest={playbackRequest}
                    onLoadingChange={setAudioLoading}
                    key={`audio-${item.id}`}
                  />
                )}
                {retranscriptionResult?.sessionId === item.id && (
                  <div style={{ marginBottom: 14 }}>
                    <HistoryResultCard
                      title={t('history.retranscribe')}
                      text={retranscriptionResult.text}
                    />
                  </div>
                )}
                {/* Pipeline detail keeps only the recognize / polish steps — left column: step
                  name, middle column: provider·model, right column: step duration/status.
                  Insertion is a foreground-delivery detail and is not shown in the note content
                  area. */}
                <div
                  style={{
                    marginBottom: 16,
                    paddingBottom: 14,
                    borderBottom: '0.5px solid var(--ol-line-soft)',
                    display: 'grid',
                    gridTemplateColumns: 'auto 1fr auto',
                    columnGap: 14,
                    rowGap: 7,
                    fontSize: 11,
                    color: 'var(--ol-ink-4)',
                    alignItems: 'baseline',
                  }}
                >
                  {(item.asrProvider || item.asrMs != null) && (
                    <>
                      <span style={{ display: 'flex' }}>
                        <Tooltip
                          content={t('history.stepAsrHint')}
                          wrap
                          placement="bottom"
                          focusable
                        >
                          <span
                            style={{
                              cursor: 'help',
                              textDecoration: 'underline dotted',
                              textDecorationColor: 'var(--ol-ink-4)',
                              textUnderlineOffset: 3,
                            }}
                          >
                            {t('history.stepAsr')}
                          </span>
                        </Tooltip>
                      </span>
                      <span
                        style={{
                          color: 'var(--ol-ink-2)',
                          fontFamily: 'var(--ol-font-mono)',
                          overflowWrap: 'anywhere',
                        }}
                      >
                        {[item.asrProvider, item.asrModel].filter(Boolean).join(' · ')}
                      </span>
                      <span
                        style={{
                          fontFamily: 'var(--ol-font-mono)',
                          textAlign: 'right',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {item.asrMs != null ? formatStepDuration(item.asrMs, t, locale) : ''}
                      </span>
                    </>
                  )}
                  {(item.llmProvider || item.llmModel || item.polishMs != null) && (
                    <>
                      <span>{t('history.stepPolish')}</span>
                      <span
                        style={{
                          color: 'var(--ol-ink-2)',
                          fontFamily: 'var(--ol-font-mono)',
                          overflowWrap: 'anywhere',
                        }}
                      >
                        {[item.llmProvider, item.llmModel].filter(Boolean).join(' · ')}
                      </span>
                      <span
                        style={{
                          fontFamily: 'var(--ol-font-mono)',
                          textAlign: 'right',
                          whiteSpace: 'nowrap',
                        }}
                      >
                        {item.polishMs != null ? formatStepDuration(item.polishMs, t, locale) : ''}
                      </span>
                    </>
                  )}
                </div>
                {/* Default to showing only the polished result; the raw text can still be
                  expanded on demand, sparing users two duplicate columns every time. */}
                <div style={{ display: 'grid', gap: 12 }}>
                  {/* The polish result box is de-blued too: neutral surface-2 fill + thin
                    outline. */}
                  <div
                    style={{
                      minWidth: 0,
                      padding: 14,
                      border: '0.5px solid var(--ol-line)',
                      borderRadius: 10,
                      background: 'var(--ol-surface-2)',
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: 8,
                        flexWrap: 'wrap',
                        marginBottom: 10,
                      }}
                    >
                      <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
                        <Pill size="sm" tone="blue" style={TRUNCATED_PILL_STYLE}>
                          {t('history.stepPolish')} · {styleLabel(item)}
                        </Pill>
                      </span>
                      <span style={{ display: 'inline-flex', gap: 6, flexShrink: 0 }}>
                        {item.rawTranscript && (
                          <Btn
                            variant="ghost"
                            size="sm"
                            onClick={() => setShowRawTranscript((visible) => !visible)}
                          >
                            {showRawTranscript
                              ? t('history.hideRaw', '隐藏原文')
                              : t('history.showRaw', '查看原文')}
                          </Btn>
                        )}
                        <Btn
                          icon={justCopied ? 'check' : 'copy'}
                          variant="ghost"
                          size="sm"
                          onClick={() => void onCopy()}
                        >
                          {justCopied ? t('common.copied') : t('common.copy')}
                        </Btn>
                      </span>
                    </div>
                    <div style={{ fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)' }}>
                      <AssistantMarkdown
                        markdown={
                          item.finalText ||
                          item.rawTranscript ||
                          t('quickNote.noTranscript', 'No transcript yet.')
                        }
                      />
                      <LearnVocabulary key={item.id} />
                    </div>
                  </div>
                  {showRawTranscript && (
                    <div
                      style={{
                        minWidth: 0,
                        padding: 14,
                        border: '0.5px solid var(--ol-line)',
                        borderRadius: 10,
                        background: 'var(--ol-surface-2)',
                      }}
                    >
                      <div
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 8,
                          marginBottom: 10,
                        }}
                      >
                        <Pill size="sm" tone="outline">
                          {t('history.stepAsr')}
                        </Pill>
                        {item.rawTranscript && (
                          <Btn
                            icon={justCopiedRaw ? 'check' : 'copy'}
                            variant="ghost"
                            size="sm"
                            onClick={() => void onCopyRaw()}
                          >
                            {justCopiedRaw ? t('common.copied') : t('common.copy')}
                          </Btn>
                        )}
                      </div>
                      <p
                        style={{
                          margin: 0,
                          fontSize: 13,
                          lineHeight: 1.7,
                          color: 'var(--ol-ink-2)',
                          whiteSpace: 'pre-wrap',
                        }}
                      >
                        {item.rawTranscript || t('history.rawEmpty')}
                      </p>
                    </div>
                  )}
                </div>
                {/* Repolish: run the LLM again on this entry's raw text. No raw text means
                  nothing to repolish (failed transcription entries) — the whole block is not
                  rendered; QA entries' raw text is the question, not text to polish, so they are
                  not rendered either. The key resets result and state when switching entries, so
                  the previous entry's result does not linger under the new one; the prefix
                  distinguishes it from the player's key above (duplicate same-level keys would
                  leave stale nodes). */}
                {repolishOpen &&
                  (item.rawTranscript.trim() || quickNotesOnly) &&
                  item.errorCode !== 'qaSession' && (
                    <RepolishPanel
                      session={item}
                      mobile={mobile}
                      allPacks={allPacks}
                      packsError={packsError}
                      onClose={() => setRepolishOpen(false)}
                      persistOnApply={quickNotesOnly}
                      onApplied={(updated) =>
                        setItems((prev) =>
                          prev.map((entry) => (entry.id === updated.id ? updated : entry)),
                        )
                      }
                      key={`repolish-${item.id}`}
                    />
                  )}
              </>
            ) : (
              <div
                style={{ padding: 40, textAlign: 'center', fontSize: 13, color: 'var(--ol-ink-4)' }}
              >
                {loading
                  ? t('common.loading')
                  : loadError
                    ? t('history.loadFailed', { err: loadError })
                    : t('history.selectHint')}
              </div>
            )}
          </Card>
        )}
      </div>
    </div>
  );
}

interface HistoryActionMenuProps {
  hasAudioRecording: boolean;
  audioLoading: boolean;
  showShare: boolean;
  canRetranscribe: boolean;
  retranscribing: boolean;
  canRepolish: boolean;
  showSaveDirectory: boolean;
  exportDirectory: string;
  onPlay: () => void;
  onExport: () => void;
  onShare: () => void;
  onRetranscribe: () => void;
  onDelete: () => void | Promise<void>;
  onRepolish: () => void;
  onChooseSaveDirectory: () => void;
  onResetSaveDirectory: () => void;
}

function HistoryActionMenu({
  hasAudioRecording,
  audioLoading,
  showShare,
  canRetranscribe,
  retranscribing,
  canRepolish,
  showSaveDirectory,
  exportDirectory,
  onPlay,
  onExport,
  onShare,
  onRetranscribe,
  onDelete,
  onRepolish,
  onChooseSaveDirectory,
  onResetSaveDirectory,
}: HistoryActionMenuProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open]);

  const run = (action: () => void | Promise<void>) => {
    setOpen(false);
    void action();
  };

  return (
    <div ref={rootRef} style={{ position: 'relative', flexShrink: 0 }}>
      <Btn
        icon="more"
        variant="ghost"
        size="sm"
        ariaLabel={t('history.actionMenu', '录音操作')}
        title={t('history.actionMenu', '录音操作')}
        ariaExpanded={open}
        onClick={() => setOpen((visible) => !visible)}
        style={{ width: 36, justifyContent: 'center', padding: '7px 8px' }}
      />
      {open && (
        <div
          role="menu"
          style={{
            position: 'absolute',
            top: 'calc(100% + 6px)',
            right: 0,
            zIndex: 30,
            width: 'min(260px, calc(100vw - 40px))',
            maxHeight: 'min(70vh, 420px)',
            overflowY: 'auto',
            padding: 6,
            border: '0.5px solid var(--ol-line-strong)',
            borderRadius: 10,
            background: 'var(--ol-surface)',
            boxShadow: 'var(--ol-shadow-md, 0 12px 30px rgba(0,0,0,.18))',
          }}
        >
          {hasAudioRecording && (
            <>
              <HistoryActionMenuItem
                icon="play"
                label={audioLoading ? t('history.audioLoading') : t('history.playRecording')}
                disabled={audioLoading}
                onClick={() => run(onPlay)}
              />
              <HistoryActionMenuItem
                icon="download"
                label={t('history.exportRecording')}
                onClick={() => run(onExport)}
              />
              {showShare && (
                <HistoryActionMenuItem
                  icon="upload"
                  label={t('quickNote.shareRecording', '分享录音')}
                  onClick={() => run(onShare)}
                />
              )}
              <div style={{ height: 1, margin: '6px 4px', background: 'var(--ol-line-soft)' }} />
            </>
          )}
          {canRetranscribe && (
            <HistoryActionMenuItem
              icon="refresh"
              label={retranscribing ? t('history.retranscribing') : t('history.retranscribe')}
              disabled={retranscribing}
              onClick={() => run(onRetranscribe)}
            />
          )}
          <HistoryActionMenuItem
            icon="sparkle"
            label={t('history.repolish.title')}
            disabled={!canRepolish}
            onClick={() => run(onRepolish)}
          />
          <HistoryActionMenuItem
            icon="trash"
            label={t('common.delete')}
            danger
            onClick={() => run(onDelete)}
          />
          {showSaveDirectory && (
            <>
              <div style={{ height: 1, margin: '6px 4px', background: 'var(--ol-line-soft)' }} />
              <HistoryActionMenuItem
                icon="archive"
                label={
                  exportDirectory
                    ? t('history.changeSaveDirectory', '更改转写文件保存位置')
                    : t('history.saveDirectory', '设置转写文件保存位置')
                }
                detail={exportDirectory || t('history.defaultSaveDirectory', '每次导出时选择')}
                onClick={() => run(onChooseSaveDirectory)}
              />
              {exportDirectory && (
                <HistoryActionMenuItem
                  icon="x"
                  label={t('history.resetSaveDirectory', '恢复默认保存位置')}
                  onClick={() => run(onResetSaveDirectory)}
                />
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}

function HistoryActionMenuItem({
  icon,
  label,
  detail,
  disabled = false,
  danger = false,
  onClick,
}: {
  icon: string;
  label: string;
  detail?: string;
  disabled?: boolean;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      style={{
        width: '100%',
        display: 'flex',
        alignItems: 'center',
        gap: 9,
        padding: '8px 9px',
        border: 0,
        borderRadius: 7,
        background: 'transparent',
        color: danger ? 'var(--ol-red, #ef4444)' : 'var(--ol-ink-2)',
        fontFamily: 'inherit',
        fontSize: 12.5,
        textAlign: 'left',
        cursor: disabled ? 'not-allowed' : 'pointer',
        opacity: disabled ? 0.5 : 1,
      }}
    >
      <Icon name={icon} size={14} />
      <span style={{ minWidth: 0, flex: 1 }}>
        <span style={{ display: 'block' }}>{label}</span>
        {detail && (
          <span
            title={detail}
            style={{
              display: 'block',
              marginTop: 2,
              overflow: 'hidden',
              color: 'var(--ol-ink-4)',
              fontSize: 10.5,
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {detail}
          </span>
        )}
      </span>
    </button>
  );
}

function historyTitle(
  session: DictationSession,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const text = (session.finalText || session.rawTranscript).trim();
  if (text) return text.split(/\r?\n/, 1)[0];
  if (session.errorCode === 'recording') return t('quickNote.recording', 'Recording…');
  if (session.errorCode === 'cancelled') {
    return t('quickNote.cancelledTitle', 'Recording cancelled');
  }
  if (session.errorCode) return t('quickNote.failedTitle', 'Recording needs attention');
  return t('quickNote.emptyTitle', 'Untitled recording');
}

/** Backend timeout errors degrade to bare strings at the IPC boundary (LLMError::Timeout →
 *  "timeout"). Only match whole-string common timeout shapes so other errors containing
 *  "timeout" are not misjudged as timeouts. */
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
  /** Key of the result card. Repeated applies of the same style overwrite the previous one
   *  instead of stacking cards forever. */
  key: string;
  title: string;
  text: string;
}

/**
 * Repolish panel: run the LLM again on this history entry's **raw text**.
 *
 * Both entries share one backend channel (`repolish`, stylePackId optional):
 * - "Retry with original style" → prefer the id of the pack that produced this entry (falling
 *   back to the currently active style when the pack is deleted / old history / not loaded).
 *   Rerunning with the same style is the control experiment that tells whether the previous
 *   result was model jitter or stable behavior — what the user really wants when they say
 *   "the AI recognized it wrong".
 * - "Apply" → pass the selected pack id, to see the same text in a different style.
 *
 * Results are shown only for this viewing session and are not written back to the history entry:
 * the entry's finalText is "what was actually inserted at the time", a factual record that a
 * later trial must not overwrite. The hint at the top of the panel says so directly.
 *
 * Note this reruns polish only, not recognition — history without an archived recording can only
 * use the raw text. The "retranscribe" entry point is open to all legacy ASR entries that still
 * have a recording.
 */
function RepolishPanel({
  session,
  mobile,
  allPacks,
  packsError,
  onClose,
  persistOnApply,
  onApplied,
}: {
  session: DictationSession;
  mobile: boolean;
  /** **All** style packs loaded at the History top level (including disabled); null = loading. */
  allPacks: StylePack[] | null;
  packsError: string | null;
  onClose: () => void;
  persistOnApply: boolean;
  onApplied: (updated: DictationSession) => void;
}) {
  const { t } = useTranslation();
  const MODE_LABEL = useModeLabel();
  const [selectedPackId, setSelectedPackId] = useState<string>('');
  const [running, setRunning] = useState<'retry' | 'apply' | null>(null);
  const [applyingKey, setApplyingKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [results, setResults] = useState<RepolishResult[]>([]);
  const canRun = session.rawTranscript.trim().length > 0;

  // List enabled packs only: disabled packs do not participate in polish elsewhere either;
  // listing them here would let "Apply" use a style the user believes is turned off.
  const packs = useMemo(() => (allPacks ? allPacks.filter((p) => p.enabled) : null), [allPacks]);

  useEffect(() => {
    if (!packs) return;
    setSelectedPackId((current) => current || defaultPackId(packs));
  }, [packs]);

  const run = async (kind: 'retry' | 'apply') => {
    // Retry prefers the original pack that produced this entry; if deleted / old history / not
    // loaded, fall back explicitly to the active pack (then the first available one) — keeping
    // the frontend label consistent with actual execution instead of letting the backend walk
    // its None fallback chain.
    const packId =
      kind === 'apply'
        ? selectedPackId
        : resolveRepolishRetryPackIdWithFallback(session, allPacks, packs ?? []);
    if (!canRun || (kind === 'apply' && !packId)) return;
    setRunning(kind);
    setError(null);
    try {
      const text = await repolish(session.rawTranscript, session.mode, packId);
      // Use allPacks rather than packs to look up the name: retrying with a disabled original
      // pack still shows its real name in the title.
      const pack = packId ? allPacks?.find((p) => p.id === packId) : undefined;
      const result: RepolishResult = {
        key: packId ?? '__retry__',
        title: pack
          ? t('history.repolish.resultTitle', { name: packDisplayName(pack, MODE_LABEL) })
          : t('history.repolish.retryResultTitle'),
        text,
      };
      // The same key overwrites the old result; a new key is prepended — the latest trial stays
      // closest to the action area.
      setResults((prev) => [result, ...prev.filter((r) => r.key !== result.key)]);
    } catch (err) {
      console.error('[history] repolish failed', err);
      const msg = errorMessage(err);
      // The backend passes LLMError::Timeout through as the bare string "timeout"; showing it
      // directly tells the user nothing — "Repolish failed: timeout" reads like the feature is
      // broken when the actual cause is the current LLM provider not responding within 30s
      // (especially common in the free model pool). Use a message the user can act on.
      setError(
        isTimeout(msg) ? t('history.repolish.timeout') : t('history.repolish.failed', { err: msg }),
      );
    } finally {
      setRunning(null);
    }
  };

  const applyResult = async (result: RepolishResult) => {
    if (!persistOnApply) return;
    setApplyingKey(result.key);
    setError(null);
    try {
      const updated = await applyQuickNoteRepolish(
        session.id,
        result.text,
        result.key === '__retry__' ? (session.stylePackId ?? undefined) : result.key,
      );
      onApplied(updated);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setApplyingKey(null);
    }
  };

  return (
    <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 10,
          flexWrap: 'wrap',
          marginBottom: 12,
        }}
      >
        <span
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            fontSize: 12,
            fontWeight: 600,
            color: 'var(--ol-ink-2)',
          }}
        >
          {t('history.repolish.title')}
          {/* Persistent hint text collapsed into a "?" badge, expanded on hover/focus (same
            pattern as the settings pages). */}
          <Tooltip content={t('history.repolish.hint')} wrap placement="bottom" focusable>
            <span
              style={{
                width: 16,
                height: 16,
                borderRadius: 999,
                background: 'var(--ol-control-muted)',
                color: 'var(--ol-ink-3)',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                cursor: 'help',
              }}
            >
              <Icon name="help" size={10} />
            </span>
          </Tooltip>
        </span>
        <span style={{ display: 'inline-flex', gap: 6 }}>
          {results.length > 0 && (
            <Btn size="sm" variant="ghost" onClick={() => setResults([])}>
              {t('history.repolish.clear')}
            </Btn>
          )}
          <Btn size="sm" variant="ghost" onClick={onClose}>
            {t('common.close')}
          </Btn>
        </span>
      </div>

      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          flexWrap: 'wrap',
          marginBottom: results.length > 0 ? 14 : 0,
        }}
      >
        {!canRun && (
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 10 }}>
            {t('quickNote.repolishNeedsTranscript', 'Re-transcribe the audio before repolishing.')}
          </div>
        )}
        <Btn
          icon="refresh"
          variant="ghost"
          size="sm"
          disabled={running !== null || !canRun}
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
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {t('history.repolish.noPacks')}
          </span>
        ) : (
          <>
            {/* Use the official SelectLite consistently; no more mixing in a native <select>. */}
            <SelectLite
              value={selectedPackId}
              onChange={setSelectedPackId}
              ariaLabel={t('history.repolish.pickStyle')}
              disabled={!packs || running !== null}
              options={(packs ?? []).map((pack) => ({
                value: pack.id,
                label: packDisplayName(pack, MODE_LABEL),
              }))}
              style={{ maxWidth: mobile ? 160 : 220, minWidth: 0, height: 30, fontSize: 13 }}
            />
            <Btn
              variant="ghost"
              size="sm"
              disabled={!selectedPackId || running !== null || !canRun}
              onClick={() => void run('apply')}
            >
              {running === 'apply' ? t('history.repolish.applying') : t('history.repolish.apply')}
            </Btn>
          </>
        )}
      </div>

      {error && (
        <div
          style={{
            marginTop: 10,
            padding: '8px 10px',
            borderRadius: 8,
            background: 'rgba(239,68,68,0.08)',
            color: 'var(--ol-red, #ef4444)',
            fontSize: 11.5,
            lineHeight: 1.45,
          }}
        >
          {error}
        </div>
      )}

      {results.length > 0 && (
        <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12 }}>
          {results.map((result) => (
            <HistoryResultCard
              key={result.key}
              title={result.title}
              text={result.text}
              applyLabel={persistOnApply ? t('quickNote.applyResult', 'Apply to note') : undefined}
              applying={applyingKey === result.key}
              onApply={persistOnApply ? () => void applyResult(result) : undefined}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function HistoryResultCard({
  title,
  text,
  applyLabel,
  applying = false,
  onApply,
}: {
  title: string;
  text: string;
  applyLabel?: string;
  applying?: boolean;
  onApply?: () => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);

  const onCopy = async () => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error('clipboard unavailable');
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (error) {
      console.error('[history] failed to copy result', error);
    }
  };

  return (
    // minWidth: 0 — grid children default to min-width: auto; a non-wrapping title Pill would
    // push the card outside the result grid (same class of problem as the detail page's two
    // column text cards).
    <div
      style={{
        minWidth: 0,
        padding: 14,
        border: '0.5px dashed var(--ol-line-strong)',
        borderRadius: 10,
        background: 'var(--ol-surface-2)',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
          marginBottom: 10,
        }}
      >
        <span style={{ display: 'flex', minWidth: 0 }} title={title}>
          <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>
            {title}
          </Pill>
        </span>
        {text.trim() && (
          <Btn
            icon={copied ? 'check' : 'copy'}
            variant="ghost"
            size="sm"
            onClick={() => void onCopy()}
          >
            {copied ? t('common.copied') : t('common.copy')}
          </Btn>
        )}
        {onApply && text.trim() && (
          <Btn variant="ghost" size="sm" disabled={applying} onClick={onApply}>
            {applying ? t('quickNote.applying', 'Applying…') : applyLabel}
          </Btn>
        )}
      </div>
      <div style={{ fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)' }}>
        <AssistantMarkdown markdown={text.trim() || t('history.repolish.empty')} />
      </div>
    </div>
  );
}

function isUserCancelled(message: string): boolean {
  const normalized = message.trim().toLowerCase();
  return (
    normalized === 'cancelled' ||
    normalized === 'canceled' ||
    normalized === 'user cancelled' ||
    normalized === 'user canceled'
  );
}

/** Rendered when session.hasAudioRecording is true: loading is triggered by the detail action
 *  menu, and once bytes arrive it switches to native audio controls. The Blob URL is revoked on
 *  unmount to avoid leaks.
 *  `onMissing` fires when the backend returns 'recording not found' (wav pruned), letting the
 *  parent hide the button for good so the user stops hitting the same error. */
function AudioRecordingPlayer({
  sessionId,
  onMissing,
  playRequest = 0,
  onLoadingChange,
}: {
  sessionId: string;
  onMissing?: () => void;
  playRequest?: number;
  onLoadingChange?: (loading: boolean) => void;
}) {
  const { t } = useTranslation();
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [errorText, setErrorText] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const blobUrlRef = useRef<string | null>(null);
  const initialPlayRequestRef = useRef(playRequest);

  // Revoke the Blob URL on unmount to avoid a memory leak.
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      onLoadingChange?.(false);
      if (blobUrlRef.current) {
        URL.revokeObjectURL(blobUrlRef.current);
        blobUrlRef.current = null;
      }
    };
  }, [onLoadingChange]);

  const clearBlobUrl = () => {
    if (blobUrlRef.current) {
      URL.revokeObjectURL(blobUrlRef.current);
      blobUrlRef.current = null;
    }
    setBlobUrl(null);
  };

  const load = useCallback(async () => {
    setStatus('loading');
    setErrorText(null);
    onLoadingChange?.(true);
    try {
      const dataUrl = await readAudioRecording(sessionId);
      if (!mountedRef.current) return;
      if (!dataUrl || dataUrl === 'data:audio/wav;base64,') throw new Error('empty recording');
      // WebKitGTK <audio> decodes data: URLs unreliably (duration 0 / won't play); decoding the
      // base64 into binary and wrapping it as a Blob URL is far more reliable under WebKit.
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
    } finally {
      onLoadingChange?.(false);
    }
  }, [onLoadingChange, onMissing, sessionId]);

  useEffect(() => {
    // A newly mounted player may receive an old global counter while the user
    // switches history entries. Only a request created after this instance
    // mounted is allowed to trigger loading/autoplay.
    if (playRequest <= initialPlayRequestRef.current) return;
    void load();
  }, [load, playRequest]);

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
  return status === 'loading' || status === 'error' ? (
    <div style={{ marginBottom: 14, fontSize: 11, color: 'var(--ol-err)' }}>
      {status === 'loading' ? t('history.audioLoading') : errorText}
    </div>
  ) : null;
}

/** Per-step pipeline duration: <1s shows whole milliseconds (streaming tails are often tens of
 *  ms; 0.1s precision would flatten different results into the same value, distorting model
 *  comparisons — PR #826 review); >=1s keeps 0.1s precision. */
function formatStepDuration(
  ms: number,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms < 1000)
    return t('common.durationMillis', { value: formatLocaleNumber(Math.round(ms), locale) });
  return formatDuration(ms, t, locale);
}

function formatDuration(
  ms: number | null,
  t: ReturnType<typeof useTranslation>['t'],
  locale: string,
): string {
  if (ms == null || ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: formatLocaleDecimal(sec, locale) });
  return t('common.durationMinutes', { value: formatLocaleDecimal(sec / 60, locale) });
}
