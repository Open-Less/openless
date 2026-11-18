import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictionaryEntry, VocabPreset } from '../../api/contracts';
import { createDeletionBatch, IncompleteDeletion } from '../../api/deletion-batch';
import {
  addVocab,
  listVocab,
  removeVocab,
  setVocabEnabled,
  updateVocab,
} from '../../api/vocabulary';
import { listenVocabularyChanges } from '../../api/vocabulary-events';
import { useExitMount } from '../../lib/useExitMount';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { DEFAULT_VOCAB_PRESETS, loadVocabPresets } from '../../lib/vocabPresets';
import { useInteraction } from '../../ui/overlays/InteractionProvider';

/** Auto-collected entries are recognized by note (the marker backend accept_pending_correction sets). */
const LEARNED_NOTE = '从手改中自动收集';

type SourceFilter = 'all' | 'auto' | 'manual';

export function useVocabularyWorkspace() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const [error, setError] = useState<string | null>(null);
  const [presets, setPresets] = useState<VocabPreset[]>(DEFAULT_VOCAB_PRESETS);

  // State added by the dictionary redesign
  const [filter, setFilter] = useState<SourceFilter>('all');
  const [query, setQuery] = useState('');
  const [wordAdded, setWordAdded] = useState(false);
  const [editingEntry, setEditingEntry] = useState<DictionaryEntry | null>(null);
  const [editDraft, setEditDraft] = useState('');
  const [editError, setEditError] = useState<string | null>(null);
  const [newWordOpen, setNewWordOpen] = useState(false);
  const [newWordDraft, setNewWordDraft] = useState('');
  const [newWordTemplateIds, setNewWordTemplateIds] = useState<string[]>([]);
  const { confirm, notify } = useInteraction('vocabulary');
  const readRequest = useRef(0);
  const deleting = useRef(false);
  const deleteRequest = useRef(false);
  const editMount = useExitMount(editingEntry !== null);
  const newWordMount = useExitMount(newWordOpen);

  // Word-grid FLIP: when add/remove/filter shifts rows, slide cards from their old
  // position to the new one; new cards enter via the .ol-word-card CSS animation and
  // confirmed deletes use a card exit animation.
  const cardRefs = useRef(new Map<string, HTMLDivElement>());
  const prevCardTops = useRef(new Map<string, number>());
  const [removingIds, setRemovingIds] = useState<Set<string>>(new Set());
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [selecting, setSelecting] = useState(false);
  const [batchBusy, setBatchBusy] = useState(false);

  const refresh = async () => {
    const request = ++readRequest.current;
    try {
      const data = await listVocab();
      if (request !== readRequest.current) return;
      setEntries(data);
      const ids = new Set(data.map((entry) => entry.id));
      setSelectedIds((current) => new Set([...current].filter((id) => ids.has(id))));
    } catch (error) {
      if (request === readRequest.current)
        setError(error instanceof Error ? error.message : String(error));
    } finally {
      if (request === readRequest.current) setLoading(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void loadVocabPresets()
      .then((value) => {
        if (!cancelled) setPresets(value);
      })
      .catch((error) => {
        if (!cancelled) setError(error instanceof Error ? error.message : String(error));
      });
    void listenVocabularyChanges(() => {
      if (!cancelled && !deleting.current) void refresh();
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((error) => console.warn('[vocabulary] could not subscribe to changes', error))
      .finally(() => {
        if (!cancelled) void refresh();
      });
    return () => {
      cancelled = true;
      readRequest.current++;
      unlisten?.();
    };
  }, []);

  const flashSaved = () => notify({ state: 'saved', message: t('common.saved') });

  const removeEntries = async (ids: string[], target: 'selected' | 'learned' | DictionaryEntry = 'selected') => {
    if (deleteRequest.current || ids.length === 0) return;
    deleteRequest.current = true;
    const remove = createDeletionBatch(ids, removeVocab);
    try {
      await confirm({
        title:
          typeof target !== 'string'
            ? t('vocab.deleteWord', { phrase: target.phrase })
            : target === 'learned'
              ? t('vocab.deleteLearned', { count: ids.length })
              : t('vocab.deleteSelected', { count: ids.length }),
        description: t('vocab.deleteScope'),
        confirmLabel: t('common.delete'),
        tone: 'danger',
        errorMessage: (error) => error instanceof IncompleteDeletion
          ? t('vocab.deleteIncomplete', { count: error.remaining })
          : t('common.operationFailed'),
        onConfirm: async () => {
          deleting.current = true;
          setBatchBusy(true);
          readRequest.current++;
          setError(null);
          try {
            await remove(async (removed) => {
              await Promise.all([...removed].map(fadeOutCard));
              setEntries((current) => current.filter((entry) => !removed.has(entry.id)));
              setSelectedIds((current) => new Set([...current].filter((id) => !removed.has(id))));
            });
            notify({ state: 'deleted', message: t('vocab.deleted') });
          } finally {
            setRemovingIds(new Set());
            deleting.current = false;
            setBatchBusy(false);
            await refresh();
          }
        },
      });
    } finally {
      deleteRequest.current = false;
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
      setWordAdded(true);
      window.setTimeout(() => setWordAdded(false), 700);
      flashSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
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

  /** Fade a card out after its deletion has been acknowledged. */
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
      /* Reconcile the view even if the card animation is interrupted. */
    }
  };

  const onRemoveAllLearnedEntries = () => removeEntries(learnedEntries.map((entry) => entry.id), 'learned');

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

  return {
    t,
    mobile,
    loading,
    error,
    presets,
    filter,
    setFilter,
    query,
    setQuery,
    wordAdded,
    setEditingEntry,
    editDraft,
    setEditDraft,
    editError,
    setEditError,
    setNewWordOpen,
    newWordDraft,
    setNewWordDraft,
    newWordTemplateIds,
    setNewWordTemplateIds,
    editMount,
    newWordMount,
    cardRefs,
    removingIds,
    selectedIds,
    setSelectedIds,
    selecting,
    setSelecting,
    batchBusy,
    removeEntries,
    toggleSelection,
    onToggle,
    openEdit,
    saveEdit,
    addNewWord,
    applyNewWordTemplates,
    sourceOf,
    learnedEntries,
    onRemoveAllLearnedEntries,
    needle,
    visibleEntries,
  };
}
