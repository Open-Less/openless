// Vocab.tsx — the "dictionary" page.
// Structure:
//   - Top: title + "new word" entry at the top-right (modal: direct input or bulk import
//     from preset templates)
//   - Toolbar: all / auto-added / manually-added segment filter + circular search on the
//     right (expands leftward on click)
//   - Word grid: cards show text only by default; hover dims them and reveals
//     "edit / delete" actions
//   - Editing uses a modal (update_vocab keeps id/hits); scene presets stay a card section
//   - Correction rules moved to the "tools → correction rules" page (Corrections.tsx)
// Data is stored in ~/Library/Application Support/OpenLess/dictionary.json (same name as Swift).

import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { Tooltip } from '../components/Tooltip';
import { SavedToast } from '../components/SavedToast';
import {
  addVocab,
  isTauri,
  listVocab,
  removeVocab,
  setVocabEnabled,
  updateVocab,
} from '../lib/ipc';
import type { DictionaryEntry, VocabPreset } from '../lib/types';
import { DEFAULT_VOCAB_PRESETS, loadVocabPresets, persistVocabPresets } from '../lib/vocabPresets';
import { useExitMount } from '../lib/useExitMount';
import { useOverlayMotion } from '../lib/motion';
import { useMobileLayout } from '../lib/useMobileLayout';
import { Btn, Card, Collapsible, PageHeader } from './_atoms';

const NEW_PRESET_DRAFT_ID = '__new__';

/** Auto-collected entries are recognized by note (the marker backend accept_pending_correction sets). */
const LEARNED_NOTE = '从手改中自动收集';

type SourceFilter = 'all' | 'auto' | 'manual';

export function Vocab() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const inputRef = useRef<HTMLInputElement>(null);

  const [error, setError] = useState<string | null>(null);
  const [presets, setPresets] = useState<VocabPreset[]>(DEFAULT_VOCAB_PRESETS);
  const [selectedPresetIds, setSelectedPresetIds] = useState<string[]>([]);
  const [editingPresetId, setEditingPresetId] = useState<string | null>(null);
  const [presetNameDraft, setPresetNameDraft] = useState('');
  const [presetPhrasesDraft, setPresetPhrasesDraft] = useState('');

  // State added by the dictionary redesign
  const [filter, setFilter] = useState<SourceFilter>('all');
  const [query, setQuery] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [editingEntry, setEditingEntry] = useState<DictionaryEntry | null>(null);
  const [editDraft, setEditDraft] = useState('');
  const [editError, setEditError] = useState<string | null>(null);
  const [newWordOpen, setNewWordOpen] = useState(false);
  const [newWordDraft, setNewWordDraft] = useState('');
  const [newWordTemplateIds, setNewWordTemplateIds] = useState<string[]>([]);
  const [saveState, setSaveState] = useState<'idle' | 'saved'>('idle');
  const editMount = useExitMount(editingEntry !== null);
  const newWordMount = useExitMount(newWordOpen);

  // Word-grid FLIP: when add/remove/filter shifts rows, slide cards from their old
  // position to the new one; new cards enter via the .ol-word-card CSS animation and
  // deletes use the exit animation in onRemove.
  const cardRefs = useRef(new Map<string, HTMLDivElement>());
  const prevCardTops = useRef(new Map<string, number>());
  const [removingIds, setRemovingIds] = useState<Set<string>>(new Set());
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [selecting, setSelecting] = useState(false);
  const [batchBusy, setBatchBusy] = useState(false);

  const refresh = async () => {
    try {
      setError(null);
      const data = await listVocab();
      setEntries(data);
      const ids = new Set(data.map((entry) => entry.id));
      setSelectedIds((current) => new Set([...current].filter((id) => ids.has(id))));
    } catch (e) {
      // Without try/catch, a backend decode failure used to leave the spinner stuck forever.
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
    void loadVocabPresets()
      .then(setPresets)
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    // Subscribe to backend vocab:updated: pushed by the coordinator after each dictation
    // segment and its record_hits. Hit counts tick up live while the Vocab page is open,
    // no tab switch away and back needed.
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const handle = await listen('vocab:updated', () => {
        void refresh();
      });
      if (cancelled) handle();
      else unlisten = handle;
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  const flashSaved = () => {
    setSaveState('saved');
    window.setTimeout(() => setSaveState('idle'), 1600);
  };

  const onAdd = async () => {
    const phrase = inputRef.current?.value.trim();
    if (!phrase) return;
    try {
      const entry = await addVocab(phrase);
      // Optimistic insertion at the head (addVocab returns the new entry, also visible
      // immediately under the browser mock).
      setEntries((prev) => [entry, ...prev]);
      flashSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    if (inputRef.current) inputRef.current.value = '';
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      void onAdd();
    }
  };

  const removeEntries = async (ids: string[]) => {
    if (batchBusy || ids.length === 0) return;
    setBatchBusy(true);
    setError(null);
    const removed = new Set<string>();
    let failures = 0;
    try {
      // Bound IPC concurrency and retain unsuccessful selections for retry.
      for (let start = 0; start < ids.length; start += 8) {
        const batch = ids.slice(start, start + 8);
        const results = await Promise.allSettled(batch.map((id) => removeVocab(id)));
        results.forEach((result, index) => {
          if (result.status === 'fulfilled') removed.add(batch[index]);
          else failures += 1;
        });
      }
      await Promise.all([...removed].map((id) => fadeOutCard(id)));
      setEntries((current) => current.filter((entry) => !removed.has(entry.id)));
      setSelectedIds((current) => new Set([...current].filter((id) => !removed.has(id))));
      if (failures) setError(t('vocab.batchDeleteFailed', { count: failures }));
      else flashSaved();
    } finally {
      setRemovingIds(new Set());
      setBatchBusy(false);
    }
  };

  const toggleSelection = (id: string) => {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const onToggle = async (entry: DictionaryEntry) => {
    const next = !entry.enabled;
    // Optimistically update the UI; on backend failure, roll back + surface the error.
    // Otherwise the UI could show "disabled" while ASR/polish still injects the entry —
    // a confusing state. issue #60.
    setEntries((prev) => prev.map((e) => (e.id === entry.id ? { ...e, enabled: next } : e)));
    try {
      await setVocabEnabled(entry.id, next);
    } catch (err) {
      setEntries((prev) =>
        prev.map((e) => (e.id === entry.id ? { ...e, enabled: entry.enabled } : e)),
      );
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const openEdit = (entry: DictionaryEntry) => {
    setEditingEntry(entry);
    setEditDraft(entry.phrase);
    setEditError(null);
  };

  const saveEdit = async () => {
    if (!editingEntry) return;
    const phrase = editDraft.trim();
    if (!phrase) {
      setEditError(t('vocab.editEmpty'));
      return;
    }
    if (phrase === editingEntry.phrase) {
      setEditingEntry(null);
      return;
    }
    try {
      await updateVocab(editingEntry.id, phrase);
      // Optimistic rename: id / hits / enabled stay unchanged (backend update_vocab edits
      // the phrase in place).
      setEntries((prev) => prev.map((e) => (e.id === editingEntry.id ? { ...e, phrase } : e)));
      setEditingEntry(null);
      flashSaved();
    } catch (err) {
      setEditError(err instanceof Error ? err.message : String(err));
    }
  };

  const addNewWord = async () => {
    const phrase = newWordDraft.trim();
    if (!phrase) return;
    try {
      const entry = await addVocab(phrase);
      setEntries((prev) => [entry, ...prev]);
      setNewWordDraft('');
      flashSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const togglePreset = (id: string) => {
    setSelectedPresetIds((prev) =>
      prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id],
    );
  };

  const startEditPreset = (preset: VocabPreset) => {
    setEditingPresetId(preset.id);
    setPresetNameDraft(preset.name);
    setPresetPhrasesDraft(preset.phrases.join(', '));
  };

  const savePreset = async () => {
    if (!editingPresetId) return;
    const name = presetNameDraft.trim();
    if (!name) return;
    const phrases = Array.from(
      new Set(
        presetPhrasesDraft
          .split(/[,\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
      ),
    );
    const next =
      editingPresetId === NEW_PRESET_DRAFT_ID
        ? [...presets, { id: `user-${Date.now()}`, name, phrases }]
        : presets.map((p) => (p.id === editingPresetId ? { ...p, name, phrases } : p));
    try {
      await persistVocabPresets(next);
      setPresets(next);
      setEditingPresetId(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const createPreset = () => {
    setEditingPresetId(NEW_PRESET_DRAFT_ID);
    setPresetNameDraft(t('vocab.presets.newPreset'));
    setPresetPhrasesDraft('');
  };

  /** Merges the selected templates' entries into the dictionary (enabling existing ones as
      needed). Returns the number of failures. */
  const applyPresets = async (selected: VocabPreset[]) => {
    const byPhrase = new Map<string, DictionaryEntry[]>();
    const addedPhrases = new Set<string>();
    for (const entry of entries) {
      const key = entry.phrase.trim().toLowerCase();
      if (!byPhrase.has(key)) byPhrase.set(key, []);
      byPhrase.get(key)?.push(entry);
    }
    let failures = 0;
    for (const p of selected) {
      for (const phrase of p.phrases) {
        const key = phrase.trim().toLowerCase();
        if (addedPhrases.has(key)) continue;
        const existing = byPhrase.get(key) || [];
        if (existing.length === 0) {
          try {
            const entry = await addVocab(phrase);
            addedPhrases.add(key);
            setEntries((prev) => [entry, ...prev]);
          } catch {
            failures += 1;
          }
          continue;
        }
        for (const item of existing) {
          if (!item.enabled) {
            try {
              await setVocabEnabled(item.id, true);
            } catch {
              failures += 1;
            }
          }
        }
      }
    }
    return failures;
  };

  const applySelectedPresets = async () => {
    const selected = presets.filter((p) => selectedPresetIds.includes(p.id));
    if (selected.length === 0) return;
    const failures = await applyPresets(selected);
    await refresh();
    if (failures > 0) {
      setError(`部分词条添加失败（${failures}）`);
    } else {
      flashSaved();
    }
  };

  const applyNewWordTemplates = async () => {
    const selected = presets.filter((p) => newWordTemplateIds.includes(p.id));
    if (selected.length === 0) return;
    const failures = await applyPresets(selected);
    setNewWordTemplateIds([]);
    setNewWordOpen(false);
    if (failures > 0) {
      setError(`部分词条添加失败（${failures}）`);
    } else {
      flashSaved();
    }
  };

  // A separate section for auto-collected entries. No badge per entry — mixed in a crowd,
  // each would have to be inspected one by one; the segment filter makes them scannable at
  // a glance, and "delete all" naturally targets only the auto section.
  // Users can always see clearly and undo in bulk — the prerequisite for trusting
  // auto-collection.
  const sourceOf = (entry: DictionaryEntry): Exclude<SourceFilter, 'all'> =>
    entry.note === LEARNED_NOTE ? 'auto' : 'manual';
  const learnedEntries = entries.filter((e) => sourceOf(e) === 'auto');

  /** Delete exit animation (reverse of entry): fade out and shrink first, delete for real
      once the animation finishes. */
  const fadeOutCard = async (id: string) => {
    const element = cardRefs.current.get(id);
    if (!element) return;
    setRemovingIds((prev) => new Set(prev).add(id));
    try {
      await element.animate(
        [
          { opacity: 1, transform: 'scale(1)' },
          { opacity: 0, transform: 'scale(0.92)' },
        ],
        { duration: 140, easing: 'ease-out', fill: 'forwards' },
      ).finished;
    } catch {
      /* Animation interrupted (filter switch/unmount) must not block deletion */
    }
  };

  const onRemoveAllLearnedEntries = () => removeEntries(learnedEntries.map((entry) => entry.id));

  const needle = query.trim().toLowerCase();
  const visibleEntries = entries.filter(
    (e) =>
      (filter === 'all' || sourceOf(e) === filter) &&
      (!needle || e.phrase.toLowerCase().includes(needle)),
  );

  // FLIP: measure layout position only (offsetTop); rect is polluted by in-flight
  // animation transforms.
  useLayoutEffect(() => {
    const nextTops = new Map<string, number>();
    cardRefs.current.forEach((element, id) => nextTops.set(id, element.offsetTop));
    cardRefs.current.forEach((element, id) => {
      const current = nextTops.get(id);
      if (current == null) return;
      const previous = prevCardTops.current.get(id);
      if (previous != null && Math.abs(previous - current) > 1) {
        element.animate(
          [{ transform: `translateY(${previous - current}px)` }, { transform: 'translateY(0)' }],
          { duration: 280, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' },
        );
      }
    });
    prevCardTops.current = nextTops;
  }, [visibleEntries]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <PageHeader
        kicker={t('vocab.kicker')}
        title={t('vocab.title')}
        desc={t('vocab.desc')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            {selectedIds.size > 0 && (
              <button
                type="button"
                className="ol-vocab-delete-selected"
                disabled={batchBusy}
                onClick={() => void removeEntries([...selectedIds])}
              >
                <Icon name="trash" size={15} />
                {t('vocab.deleteSelected', { count: selectedIds.size })}
              </button>
            )}
            <Btn
              variant="primary"
              icon="plus"
              onClick={() => {
                setNewWordOpen(true);
                setNewWordDraft('');
                setNewWordTemplateIds([]);
              }}
            >
              {t('vocab.newWord')}
            </Btn>
          </div>
        }
      />

      <SavedToast saveState={saveState} message={t('common.saved')} />

      {/* Toolbar: source segment filter + circular search (expands leftward on click). */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          marginBottom: 14,
          flexWrap: 'wrap',
          flexShrink: 0,
        }}
      >
        <div className="ol-seg" role="tablist" aria-label={t('vocab.title')}>
          {(
            [
              { id: 'all', icon: null },
              { id: 'auto', icon: 'sparkle' },
              { id: 'manual', icon: 'feather' },
            ] as const
          ).map((seg) => (
            <button
              key={seg.id}
              type="button"
              role="tab"
              aria-selected={filter === seg.id}
              className={filter === seg.id ? 'ol-seg-item ol-seg-item-active' : 'ol-seg-item'}
              onClick={() => setFilter(seg.id)}
            >
              {seg.icon && <Icon name={seg.icon} size={13} />}
              {t(`vocab.filter.${seg.id}`)}
            </button>
          ))}
        </div>
        <Btn
          size="sm"
          onClick={() => {
            setSelecting((current) => {
              if (current) setSelectedIds(new Set());
              return !current;
            });
          }}
        >
          {selecting ? t('vocab.doneSelecting') : t('vocab.selecting')}
        </Btn>
        <label className="ol-vocab-select-all" hidden={!selecting}>
          <input
            type="checkbox"
            disabled={batchBusy || visibleEntries.length === 0}
            checked={
              visibleEntries.length > 0 &&
              visibleEntries.every((entry) => selectedIds.has(entry.id))
            }
            ref={(element) => {
              if (element)
                element.indeterminate =
                  visibleEntries.some((entry) => selectedIds.has(entry.id)) &&
                  !visibleEntries.every((entry) => selectedIds.has(entry.id));
            }}
            onChange={(event) => {
              const checked = event.target.checked;
              setSelectedIds((current) => {
                const next = new Set(current);
                visibleEntries.forEach((entry) =>
                  checked ? next.add(entry.id) : next.delete(entry.id),
                );
                return next;
              });
            }}
          />
          {selectedIds.size
            ? t('vocab.selectedCount', { count: selectedIds.size })
            : t('vocab.selectAllVisible')}
        </label>
        <div style={{ flex: 1 }} />
        {/* The circular control expands in place into the search box — the magnifier stays
            fixed at the right edge, the "search" placeholder sits inside the box; collapse
            uses the same width transition (reverse of expansion). */}
        <div className={searchOpen ? 'ol-search ol-search-open' : 'ol-search'}>
          <input
            className="ol-search-field"
            type="text"
            value={query}
            placeholder={t('vocab.searchPlaceholder')}
            aria-label={t('vocab.searchPlaceholder')}
            tabIndex={searchOpen ? 0 : -1}
            onChange={(e) => setQuery(e.target.value)}
            onBlur={() => {
              if (!query) setSearchOpen(false);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setQuery('');
                setSearchOpen(false);
              }
            }}
          />
          <button
            type="button"
            className="ol-search-icon"
            aria-label={t('vocab.searchPlaceholder')}
            aria-expanded={searchOpen}
            // Don't let the icon click blur the input: otherwise the blur-collapse and the
            // click toggle race — a second click would collapse first and then be re-opened
            // by the toggle, never collapsing.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              if (searchOpen && query) {
                setQuery('');
                return;
              }
              setSearchOpen((prev) => !prev);
              if (!searchOpen) {
                window.setTimeout(() => inputRefSearchFocus(), 60);
              } else {
                // After collapsing, the input is invisible; don't leave focus in it.
                const active = document.activeElement;
                if (active instanceof HTMLElement && active.closest('.ol-search')) active.blur();
              }
            }}
          >
            <Icon name="search" size={15} />
          </button>
        </div>
      </div>

      {error && (
        <div
          role="alert"
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: 10,
            padding: '8px 12px',
            marginBottom: 12,
            borderRadius: 10,
            border: '0.5px solid rgba(239,68,68,0.22)',
            background: 'rgba(239,68,68,0.07)',
            color: 'var(--ol-red, #ef4444)',
            fontSize: 12,
            lineHeight: 1.5,
          }}
        >
          <span>{error}</span>
        </div>
      )}

      {/* Under the auto-added filter, give "delete all" a stable outlet (the trust
          prerequisite; see the comment above). */}
      {filter === 'auto' && learnedEntries.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
          <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            {t('vocab.learnedSection', { count: learnedEntries.length })}
          </span>
          <Btn size="sm" onClick={() => void onRemoveAllLearnedEntries()}>
            {t('vocab.removeAllLearned')}
          </Btn>
        </div>
      )}

      {/* Independent scroll region: with many entries only this cell scrolls, while the
          bottom panel stays pinned to the viewport bottom edge with an opaque background
          that naturally covers content scrolling beneath; this also eliminates at the root
          the previous glitch where the grid, as a page-root flex item, got squashed and its
          content overflowed behind the section below. */}
      <div
        className="ol-thinscroll"
        style={{ flex: 1, minHeight: 0, overflowY: 'auto', paddingRight: 2, paddingBottom: 12 }}
      >
        {/* Word grid: hover dims cards and reveals edit/delete on the right. */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: mobile
              ? 'minmax(0, 1fr)'
              : 'repeat(auto-fill, minmax(230px, 1fr))',
            gap: 10,
            minHeight: 80,
            alignContent: 'start',
          }}
        >
          {loading && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
          )}
          {!loading && !error && visibleEntries.length === 0 && (
            <div style={{ fontSize: 12, color: 'var(--ol-ink-4)', gridColumn: '1 / -1' }}>
              {needle ? t('vocab.searchEmpty') : t('vocab.empty')}
            </div>
          )}
          {visibleEntries.map((entry) => (
            <WordCard
              key={entry.id}
              entry={entry}
              auto={sourceOf(entry) === 'auto'}
              removing={removingIds.has(entry.id) || batchBusy}
              selected={selectedIds.has(entry.id)}
              selecting={selecting}
              onSelect={() => toggleSelection(entry.id)}
              cardRef={(element) => {
                if (element) cardRefs.current.set(entry.id, element);
                else cardRefs.current.delete(entry.id);
              }}
              onToggle={() => void onToggle(entry)}
              onEdit={() => openEdit(entry)}
            />
          ))}
        </div>
      </div>

      {/* Bottom panel: quick-add row + hints + scene presets as one fixed block. When the
          scene presets expand, the panel grows taller upward while the gap between the input
          row and the panel's top edge stays constant. */}
      <div
        style={{
          flexShrink: 0,
          paddingTop: 14,
          background: 'var(--ol-surface)',
          boxShadow: '0 -18px 22px -18px rgba(15,17,22,0.14)',
        }}
      >
        {/* Quick-add row (keeps the original type-and-add flow). */}
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            ref={inputRef}
            placeholder={t('vocab.placeholder')}
            onKeyDown={onKeyDown}
            style={{
              flex: 1,
              height: 36,
              padding: '0 12px',
              border: '0.5px solid var(--ol-line-strong)',
              borderRadius: 999,
              fontSize: 13,
              fontFamily: 'inherit',
              outline: 'none',
              background: 'var(--ol-surface-2)',
              transition:
                'border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft), background 0.16s var(--ol-motion-quick)',
            }}
          />
          <Btn variant="primary" icon="plus" onClick={onAdd}>
            {t('common.add')}
          </Btn>
        </div>
        <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('vocab.tip')}</div>

        {/* Scene presets: card section (shares the same template data as the "new word"
            modal). Expandable; a divider on top separates it from the input area. */}
        <div
          style={{
            marginTop: 12,
            paddingTop: 12,
            borderTop: '0.5px solid var(--ol-line)',
            paddingBottom: 8,
          }}
        >
          <Card padding={0}>
            <Collapsible embedded title={t('vocab.presets.title')} desc={t('vocab.presets.tip')}>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
                {presets.map((p) => (
                  <button
                    key={p.id}
                    onClick={() => togglePreset(p.id)}
                    style={{
                      border: '0.5px solid var(--ol-line-strong)',
                      borderRadius: 999,
                      padding: '4px 10px',
                      fontSize: 12,
                      background: selectedPresetIds.includes(p.id)
                        ? 'var(--ol-blue-soft)'
                        : 'var(--ol-surface-2)',
                    }}
                  >
                    {p.name}
                  </button>
                ))}
                <Btn size="sm" variant="ghost" onClick={createPreset}>
                  {t('vocab.presets.create')}
                </Btn>
                <Btn size="sm" variant="primary" onClick={applySelectedPresets}>
                  {t('vocab.presets.apply')}
                </Btn>
              </div>
              {editingPresetId && (
                <div style={{ marginTop: 10, display: 'grid', gap: 8 }}>
                  <input
                    value={presetNameDraft}
                    onChange={(e) => setPresetNameDraft(e.target.value)}
                    placeholder={t('vocab.presets.namePlaceholder')}
                  />
                  <textarea
                    value={presetPhrasesDraft}
                    onChange={(e) => setPresetPhrasesDraft(e.target.value)}
                    placeholder={t('vocab.presets.wordsPlaceholder')}
                    rows={3}
                  />
                  <div style={{ display: 'flex', gap: 8 }}>
                    <Btn size="sm" variant="primary" onClick={() => void savePreset()}>
                      {t('vocab.presets.save')}
                    </Btn>
                    <Btn size="sm" variant="ghost" onClick={() => setEditingPresetId(null)}>
                      {t('common.cancel')}
                    </Btn>
                  </div>
                </div>
              )}
              {!editingPresetId && presets.length > 0 && (
                <div style={{ marginTop: 10, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {presets.map((p) => (
                    <Btn
                      key={`${p.id}-edit`}
                      size="sm"
                      variant="ghost"
                      onClick={() => startEditPreset(p)}
                    >
                      {t('vocab.presets.edit', { name: p.name })}
                    </Btn>
                  ))}
                </div>
              )}
            </Collapsible>
          </Card>
        </div>
      </div>

      {/* Edit-entry modal */}
      {editMount.mounted && (
        <ModalShell
          title={t('vocab.editTitle')}
          closing={editMount.closing}
          onClose={() => setEditingEntry(null)}
        >
          <input
            autoFocus
            value={editDraft}
            onChange={(e) => {
              setEditDraft(e.target.value);
              setEditError(null);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                void saveEdit();
              }
            }}
            style={{
              width: '100%',
              boxSizing: 'border-box',
              height: 40,
              padding: '0 12px',
              border: '1.5px solid var(--ol-ink)',
              borderRadius: 10,
              fontSize: 14,
              fontFamily: 'inherit',
              outline: 'none',
              background: 'var(--ol-surface)',
              color: 'var(--ol-ink)',
            }}
          />
          {editError && (
            <div style={{ marginTop: 8, fontSize: 12, color: 'var(--ol-red, #ef4444)' }}>
              {editError}
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
            <Btn variant="ghost" onClick={() => setEditingEntry(null)}>
              {t('common.cancel')}
            </Btn>
            <Btn variant="primary" onClick={() => void saveEdit()}>
              {t('vocab.editSave')}
            </Btn>
          </div>
        </ModalShell>
      )}

      {/* New-word modal: direct input + preset template multi-select */}
      {newWordMount.mounted && (
        <ModalShell
          title={t('vocab.newWordTitle')}
          desc={t('vocab.newWordDesc')}
          closing={newWordMount.closing}
          onClose={() => setNewWordOpen(false)}
        >
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              autoFocus
              value={newWordDraft}
              onChange={(e) => setNewWordDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  void addNewWord();
                }
              }}
              placeholder={t('vocab.newWordInputPlaceholder')}
              style={{
                flex: 1,
                minWidth: 0,
                height: 38,
                padding: '0 12px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 999,
                fontSize: 13.5,
                fontFamily: 'inherit',
                outline: 'none',
                background: 'var(--ol-surface-2)',
                color: 'var(--ol-ink)',
              }}
            />
            <Btn variant="primary" icon="plus" onClick={() => void addNewWord()}>
              {t('common.add')}
            </Btn>
          </div>
          <div style={{ marginTop: 16, fontSize: 12.5, fontWeight: 600, color: 'var(--ol-ink-2)' }}>
            {t('vocab.newWordTemplates')}
          </div>
          <div style={{ marginTop: 8, display: 'grid', gap: 8 }}>
            {presets.map((p) => {
              const checked = newWordTemplateIds.includes(p.id);
              return (
                <button
                  key={p.id}
                  type="button"
                  aria-pressed={checked}
                  onClick={() =>
                    setNewWordTemplateIds((prev) =>
                      prev.includes(p.id) ? prev.filter((x) => x !== p.id) : [...prev, p.id],
                    )
                  }
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 10,
                    textAlign: 'left',
                    padding: '10px 12px',
                    borderRadius: 10,
                    fontFamily: 'inherit',
                    border: checked
                      ? '1px solid var(--ol-blue)'
                      : '0.5px solid var(--ol-line-strong)',
                    background: checked ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
                    cursor: 'default',
                    transition:
                      'background 0.14s var(--ol-motion-quick), border-color 0.14s var(--ol-motion-quick)',
                  }}
                >
                  <span style={{ minWidth: 0, flex: 1 }}>
                    <span
                      style={{
                        display: 'block',
                        fontSize: 13,
                        fontWeight: 600,
                        color: 'var(--ol-ink)',
                      }}
                    >
                      {p.name}
                    </span>
                    <span
                      style={{
                        display: 'block',
                        marginTop: 2,
                        fontSize: 11.5,
                        color: 'var(--ol-ink-4)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {p.phrases.join(' · ')}
                    </span>
                  </span>
                  <span style={{ flexShrink: 0, fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
                    {t('vocab.newWordTemplateCount', { count: p.phrases.length })}
                  </span>
                  {checked && <Icon name="check" size={14} />}
                </button>
              );
            })}
          </div>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 18 }}>
            <Btn variant="ghost" onClick={() => setNewWordOpen(false)}>
              {t('common.close')}
            </Btn>
            <Btn
              variant="primary"
              onClick={() => void applyNewWordTemplates()}
              disabled={newWordTemplateIds.length === 0}
            >
              {t('vocab.newWordAddSelected')}
            </Btn>
          </div>
        </ModalShell>
      )}

      <style>{`
        @keyframes ol-chip-in {
          from { opacity: 0; transform: scale(.97); }
          to   { opacity: 1; transform: scale(1); }
        }
      `}</style>
    </div>
  );
}

function inputRefSearchFocus() {
  const el = document.querySelector<HTMLInputElement>('.ol-search-field');
  el?.focus();
}

interface WordCardProps {
  entry: DictionaryEntry;
  auto: boolean;
  /** Delete exit animation in progress: block interaction to prevent repeated clicks. */
  removing: boolean;
  cardRef: (element: HTMLDivElement | null) => void;
  onToggle: () => void;
  onEdit: () => void;
  selected: boolean;
  selecting: boolean;
  onSelect: () => void;
}

/** Word card: shows icon + text + hit count by default; hover/focus-within dims it and
    reveals edit/delete. */
function WordCard({
  entry,
  auto,
  removing,
  selected,
  selecting,
  cardRef,
  onToggle,
  onEdit,
  onSelect,
}: WordCardProps) {
  const { t } = useTranslation();
  const enabled = entry.enabled;
  return (
    <div
      ref={cardRef}
      className="ol-word-card"
      data-disabled={enabled ? undefined : 'true'}
      data-selected={selected ? 'true' : undefined}
      data-selecting={selecting ? 'true' : undefined}
      style={removing ? { pointerEvents: 'none' } : undefined}
    >
      <span className="ol-word-card-icon" aria-hidden>
        <Icon name={auto ? 'sparkle' : 'feather'} size={14} />
      </span>
      <button type="button" className="ol-word-card-text" onClick={onToggle} title={entry.phrase}>
        {entry.phrase}
      </button>
      {!enabled && <span className="ol-word-card-state">{t('vocab.disabledWord')}</span>}
      <span className="ol-word-card-hits">{entry.hits}</span>
      <span className="ol-word-card-actions">
        <Tooltip content={t('vocab.edit')} placement="top">
          <button
            type="button"
            className="ol-word-card-action"
            aria-label={t('vocab.edit')}
            onClick={onEdit}
          >
            <Icon name="pencil" size={14} />
          </button>
        </Tooltip>
      </span>
      <input
        type="checkbox"
        className="ol-word-card-select"
        checked={selected}
        disabled={removing}
        aria-label={t('vocab.selectWord', { phrase: entry.phrase })}
        onChange={onSelect}
      />
    </div>
  );
}

interface ModalShellProps {
  title: string;
  desc?: string;
  /** Keeps the closing card mounted until its exit finishes. */
  closing?: boolean;
  onClose: () => void;
  children: React.ReactNode;
}

/** Page-level mini modal: backdrop fade-in + card spring pop-out; closes on Esc or backdrop click. */
function ModalShell({ title, desc, closing = false, onClose, children }: ModalShellProps) {
  const { t } = useTranslation();
  const overlayRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop');
  useOverlayMotion(cardRef, closing);
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [onClose]);
  return (
    <div
      ref={overlayRef}
      onClick={closing ? undefined : onClose}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 80,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 24,
        background: 'var(--ol-dialog-backdrop)',
        backdropFilter: 'blur(6px) saturate(140%)',
        WebkitBackdropFilter: 'blur(6px) saturate(140%)',
        pointerEvents: closing ? 'none' : undefined,
      }}
    >
      <div
        ref={cardRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 440,
          maxWidth: '100%',
          borderRadius: 'var(--ol-dialog-radius)',
          background: 'var(--ol-surface)',
          border: '1px solid var(--ol-dialog-border)',
          boxShadow: 'var(--ol-dialog-shadow)',
          padding: 20,
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'flex-start',
            justifyContent: 'space-between',
            gap: 12,
            marginBottom: desc ? 4 : 14,
          }}
        >
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 15, fontWeight: 650, color: 'var(--ol-ink)' }}>{title}</div>
            {desc && (
              <div
                style={{ marginTop: 4, fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}
              >
                {desc}
              </div>
            )}
          </div>
          <button
            type="button"
            aria-label={t('common.close')}
            onClick={onClose}
            style={{
              width: 26,
              height: 26,
              flexShrink: 0,
              border: 0,
              borderRadius: 8,
              background: 'transparent',
              color: 'var(--ol-ink-4)',
              cursor: 'default',
              display: 'inline-flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            <Icon name="close" size={14} />
          </button>
        </div>
        <div style={{ marginTop: 12 }}>{children}</div>
      </div>
    </div>
  );
}
