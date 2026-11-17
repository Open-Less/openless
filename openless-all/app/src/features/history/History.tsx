import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession, StylePack } from '../../api/contracts';
import { clearHistory } from '../../api/history';
import { formatComboLabel } from '../../api/hotkeys';
import { listStylePacks } from '../../api/style-packs';
import { historyStore, useHistoryRecords } from '../../data/history';
import { useHotkeySettings } from '../../data/preferences';
import { getRuntimeEnvironment, getShortcutPlatform } from '../../runtime/environment';
import { Btn, Card, PageHeader } from '../../pages/_atoms';
import { useModalActive } from '../../ui/layout/AppViewport';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { HistoryDetail } from './HistoryDetail';
import { HistoryList } from './HistoryList';
import { errorMessage, styleLabelFor, useModeLabel } from './presentation';

const emptyHistory: DictationSession[] = [];

export function History({ quickNotesOnly = false }: { quickNotesOnly?: boolean } = {}) {
  const { t, i18n } = useTranslation();
  const { confirm, notify } = useInteraction(quickNotesOnly ? 'quick-note' : 'history');
  const modalActive = useModalActive();
  const { prefs } = useHotkeySettings();
  const modeLabel = useModeLabel();
  const snapshot = useHistoryRecords();
  const shortKeys = getShortcutPlatform();
  const mobileHost = getRuntimeEnvironment().isMobile;

  const items = useMemo(
    () =>
      quickNotesOnly
        ? (snapshot.data?.filter((entry) => entry.source === 'quick_note') ?? emptyHistory)
        : (snapshot.data ?? emptyHistory),
    [quickNotesOnly, snapshot.data],
  );
  const loading = snapshot.data === null && snapshot.loading;
  const loadError = snapshot.error === null ? null : errorMessage(snapshot.error);
  const [actionError, setActionError] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const [mobile, setMobile] = useState(mobileHost);
  const [mobileDetailOpen, setMobileDetailOpen] = useState(false);
  const refreshedByUser = useRef(false);
  const clearRequest = useRef(false);

  useLayoutEffect(() => {
    const panel = grid.current;
    if (!panel) return;
    const fit = (width: number) => {
      const next = mobileHost || width < 690;
      setMobile(next);
      if (!next) setMobileDetailOpen(true);
    };
    fit(panel.clientWidth);
    if (typeof ResizeObserver !== 'function') return;
    const observer = new ResizeObserver(([entry]) => fit(entry.contentRect.width));
    observer.observe(panel);
    return () => observer.disconnect();
  }, [mobileHost]);

  const [allPacks, setAllPacks] = useState<StylePack[] | null>(null);
  const [packsError, setPacksError] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void listStylePacks().then(
      (packs) => {
        if (!cancelled) setAllPacks(packs);
      },
      (error: unknown) => {
        if (!cancelled) setPacksError(errorMessage(error));
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  const refresh = useCallback(
    async (withNotice: boolean) => {
      if (withNotice && refreshedByUser.current) return;
      if (withNotice) refreshedByUser.current = true;
      try {
        const result = await historyStore.refreshRecords();
        if (withNotice) {
          notify(
            result.error === null
              ? { message: t('common.refreshed'), state: 'saved' }
              : { message: t('common.operationFailed'), state: 'failed', durationMs: 3500 },
          );
        }
      } finally {
        if (withNotice) refreshedByUser.current = false;
      }
    },
    [notify, t],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedQuery(query), 300);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    if (modalActive) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.altKey || event.shiftKey) return;
      if (!(shortKeys.isMac ? event.metaKey : event.ctrlKey)) return;
      if (event.key.toLowerCase() === 'k' && searchInputRef.current) {
        event.preventDefault();
        searchInputRef.current.focus();
      } else if (event.key.toLowerCase() === 'r') {
        event.preventDefault();
        void refresh(true);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [modalActive, refresh, shortKeys.isMac]);

  const filtered = useMemo(() => {
    const term = debouncedQuery.trim().toLowerCase();
    if (!term) return items;
    return items.filter(
      ({ rawTranscript, finalText }) =>
        rawTranscript.toLowerCase().includes(term) || finalText.toLowerCase().includes(term),
    );
  }, [items, debouncedQuery]);
  const item = filtered.find((entry) => entry.id === selectedId) ?? filtered[0];
  const styleLabel = (entry: DictationSession) => styleLabelFor(entry, allPacks, modeLabel);
  const clearableCount = items.filter((entry) => entry.source !== 'quick_note').length;
  const binding = quickNotesOnly ? prefs?.quickNoteHotkey : prefs?.dictationHotkey;

  const onClear = async () => {
    if (!clearableCount || clearRequest.current) return;
    clearRequest.current = true;
    try {
      if (
        !(await confirm({
          title: t('common.clear'),
          description: t('history.confirmClear', { count: clearableCount }),
          confirmLabel: t('common.clear'),
          cancelLabel: t('common.cancel'),
          icon: 'trash',
          tone: 'danger',
        }))
      )
        return;
      setClearing(true);
      setActionError(null);
      await clearHistory();
      notify({
        message: t('history.cleared', {
          defaultValue: (i18n.resolvedLanguage ?? i18n.language).startsWith('zh')
            ? '已清空历史'
            : 'History cleared',
        }),
        state: 'deleted',
      });
    } catch (error) {
      const message = t('history.clearFailed', { err: errorMessage(error) });
      setActionError(message);
      notify({ message, state: 'failed', durationMs: 3500 });
    } finally {
      clearRequest.current = false;
      setClearing(false);
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
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh(true)}>
              {t('common.refresh')}
            </Btn>
            {!quickNotesOnly && (
              <Btn
                icon="trash"
                variant="ghost"
                size="sm"
                onClick={() => void onClear()}
                disabled={clearing || clearableCount === 0}
              >
                {t('common.clear')}
              </Btn>
            )}
          </div>
        }
      />
      <div
        ref={grid}
        style={{
          display: 'grid',
          gridTemplateColumns: mobile ? 'minmax(0, 1fr)' : 'minmax(220px, 300px) minmax(0, 1fr)',
          gap: 14,
          flex: 1,
          minHeight: 0,
        }}
      >
        {(!mobile || !mobileDetailOpen) && (
          <HistoryList
            searchInputRef={searchInputRef}
            searchShortcut={shortKeys.isMac ? '⌘K' : 'Ctrl+K'}
            query={query}
            debouncedQuery={debouncedQuery}
            onQuery={setQuery}
            filtered={filtered}
            selectedId={item?.id}
            onSelect={(id) => {
              setSelectedId(id);
              if (mobile) setMobileDetailOpen(true);
            }}
            trigger={binding ? formatComboLabel(binding) : ''}
            actionError={actionError}
            loading={loading}
            loadError={loadError}
            refresh={refresh}
            styleLabel={styleLabel}
          />
        )}
        {(!mobile || mobileDetailOpen) &&
          (item ? (
            <HistoryDetail
              key={item.id}
              item={item}
              mobile={mobile}
              onBack={() => setMobileDetailOpen(false)}
              quickNotesOnly={quickNotesOnly}
              styleLabel={styleLabel}
              allPacks={allPacks}
              packsError={packsError}
              notify={notify}
            />
          ) : (
            <Card padding={20} className="ol-thinscroll" style={{ overflow: 'auto' }}>
              {mobile && (
                <Btn
                  icon="chevLeft"
                  variant="ghost"
                  size="sm"
                  onClick={() => setMobileDetailOpen(false)}
                >
                  {t('history.backToList')}
                </Btn>
              )}
              <p
                style={{ padding: 40, textAlign: 'center', fontSize: 13, color: 'var(--ol-ink-4)' }}
              >
                {loading
                  ? t('common.loading')
                  : loadError
                    ? t('history.loadFailed', { err: loadError })
                    : t('history.selectHint')}
              </p>
            </Card>
          ))}
      </div>
    </div>
  );
}
