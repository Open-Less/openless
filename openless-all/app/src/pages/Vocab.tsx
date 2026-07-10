// Vocab.tsx — 接 Tauri 后端 list_vocab / add_vocab / remove_vocab / set_vocab_enabled。
// 数据落地到 ~/Library/Application Support/OpenLess/dictionary.json（与 Swift 同名）。

import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  addCorrectionRule,
  addVocab,
  isTauri,
  listCorrectionRules,
  listVocab,
  removeCorrectionRule,
  removeVocab,
  setCorrectionRuleEnabled,
  setVocabEnabled,
} from '../lib/ipc';
import type { CorrectionRule, DictionaryEntry, VocabPreset } from '../lib/types';
import { DEFAULT_VOCAB_PRESETS, loadVocabPresets, persistVocabPresets } from '../lib/vocabPresets';
import { useMobileLayout } from '../lib/useMobileLayout';
import { Btn, Card, Collapsible, PageHeader } from './_atoms';

const NEW_PRESET_DRAFT_ID = '__new__';
const NUM_TOKEN = '{num}';

function isSupportedCorrectionRule(pattern: string, replacement: string) {
  const tokenCount = pattern.split(NUM_TOKEN).length - 1;
  if (!pattern) return false;
  if (tokenCount > 1) return false;
  if (replacement.includes(NUM_TOKEN) && tokenCount === 0) return false;
  if (tokenCount === 1) {
    const [prefix, suffix] = pattern.split(NUM_TOKEN);
    return Boolean(prefix || suffix);
  }
  return true;
}

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
  const [correctionRules, setCorrectionRules] = useState<CorrectionRule[]>([]);
  const [rulePatternDraft, setRulePatternDraft] = useState('');
  const [ruleReplacementDraft, setRuleReplacementDraft] = useState('');

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

  const refreshCorrectionRules = async () => {
    try {
      const data = await listCorrectionRules();
      setCorrectionRules(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const refreshAll = () => {
    void refresh();
    void refreshCorrectionRules();
  };

  useEffect(() => {
    refreshAll();
    void loadVocabPresets()
      .then(setPresets)
      .catch(err => setError(err instanceof Error ? err.message : String(err)));
    // 订阅后端 vocab:updated：每段口述结束、record_hits 触发后由 coordinator 推送。
    // Vocab 页面打开期间能即时看到命中数累加，无需切到其他 tab 再切回。
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

  const onAddCorrectionRule = async () => {
    const pattern = rulePatternDraft.trim();
    if (!pattern) return;
    const replacement = ruleReplacementDraft.trim();
    if (!isSupportedCorrectionRule(pattern, replacement)) {
      setError(t('vocab.corrections.invalid'));
      return;
    }
    try {
      const rule = await addCorrectionRule(pattern, replacement);
      setCorrectionRules(prev => [rule, ...prev]);
      setRulePatternDraft('');
      setRuleReplacementDraft('');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onRemoveCorrectionRule = async (id: string) => {
    try {
      await removeCorrectionRule(id);
      setCorrectionRules(prev => prev.filter(r => r.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onToggleCorrectionRule = async (rule: CorrectionRule) => {
    const next = !rule.enabled;
    setCorrectionRules(prev => prev.map(r => (r.id === rule.id ? { ...r, enabled: next } : r)));
    try {
      await setCorrectionRuleEnabled(rule.id, next);
    } catch (err) {
      setCorrectionRules(prev => prev.map(r => (r.id === rule.id ? { ...r, enabled: rule.enabled } : r)));
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onRemove = async (id: string) => {
    await removeVocab(id);
    setEntries(prev => prev.filter(e => e.id !== id));
  };

  const onToggle = async (entry: DictionaryEntry) => {
    const next = !entry.enabled;
    // 乐观更新 UI；后端失败时回滚 + 让用户看到错误，避免 UI 显示「已禁用」但 ASR/polish
    // 仍在注入此词条造成的诡异状态。issue #60。
    setEntries(prev => prev.map(e => (e.id === entry.id ? { ...e, enabled: next } : e)));
    try {
      await setVocabEnabled(entry.id, next);
    } catch (err) {
      setEntries(prev => prev.map(e => (e.id === entry.id ? { ...e, enabled: entry.enabled } : e)));
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const togglePreset = (id: string) => {
    setSelectedPresetIds(prev => (prev.includes(id) ? prev.filter(x => x !== id) : [...prev, id]));
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
          .map(s => s.trim())
          .filter(Boolean),
      ),
    );
    const next =
      editingPresetId === NEW_PRESET_DRAFT_ID
        ? [...presets, { id: `user-${Date.now()}`, name, phrases }]
        : presets.map(p => (p.id === editingPresetId ? { ...p, name, phrases } : p));
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

  const applySelectedPresets = async () => {
    const selected = presets.filter(p => selectedPresetIds.includes(p.id));
    if (selected.length === 0) return;
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
            await addVocab(phrase);
            addedPhrases.add(key);
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
    await refresh();
    if (failures > 0) {
      setError(`部分词条添加失败（${failures}）`);
    }
  };

  return (
    <>
      <PageHeader
        kicker={t('vocab.kicker')}
        title={t('vocab.title')}
        desc={t('vocab.desc')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={refreshAll}>{t('common.refresh')}</Btn>
          </div>
        }
      />
      <Card padding={0}>
        <Collapsible
          embedded
          title={t('vocab.presets.title')}
          desc={t('vocab.presets.tip')}
        >
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            {presets.map(p => (
              <button
                key={p.id}
                onClick={() => togglePreset(p.id)}
                style={{
                  border: '0.5px solid var(--ol-line-strong)',
                  borderRadius: 999,
                  padding: '4px 10px',
                  fontSize: 12,
                  background: selectedPresetIds.includes(p.id) ? 'var(--ol-blue-soft)' : 'var(--ol-surface-2)',
                }}
              >
                {p.name}
              </button>
            ))}
            <Btn size="sm" variant="ghost" onClick={createPreset}>{t('vocab.presets.create')}</Btn>
            <Btn size="sm" variant="primary" onClick={applySelectedPresets}>{t('vocab.presets.apply')}</Btn>
          </div>
          {editingPresetId && (
            <div style={{ marginTop: 10, display: 'grid', gap: 8 }}>
              <input value={presetNameDraft} onChange={e => setPresetNameDraft(e.target.value)} placeholder={t('vocab.presets.namePlaceholder')} />
              <textarea value={presetPhrasesDraft} onChange={e => setPresetPhrasesDraft(e.target.value)} placeholder={t('vocab.presets.wordsPlaceholder')} rows={3} />
              <div style={{ display: 'flex', gap: 8 }}>
                <Btn size="sm" variant="primary" onClick={() => void savePreset()}>{t('vocab.presets.save')}</Btn>
                <Btn size="sm" variant="ghost" onClick={() => setEditingPresetId(null)}>{t('common.cancel')}</Btn>
              </div>
            </div>
          )}
          {!editingPresetId && presets.length > 0 && (
            <div style={{ marginTop: 10, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              {presets.map(p => (
                <Btn key={`${p.id}-edit`} size="sm" variant="ghost" onClick={() => startEditPreset(p)}>
                  {t('vocab.presets.edit', { name: p.name })}
                </Btn>
              ))}
            </div>
          )}
        </Collapsible>
        <Collapsible
          embedded
          title={t('vocab.corrections.title')}
          desc={t('vocab.corrections.tip')}
        >
          <div style={{ display: 'grid', gap: 10 }}>
            <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : 'minmax(0, 1fr) auto minmax(0, 1fr) auto', gap: 8, alignItems: mobile ? 'stretch' : 'center' }}>
              <input
                value={rulePatternDraft}
                onChange={e => setRulePatternDraft(e.target.value)}
                placeholder={t('vocab.corrections.patternPlaceholder')}
                style={{ height: 32, padding: '0 10px', border: '0.5px solid var(--ol-line-strong)', borderRadius: 8, background: 'var(--ol-surface-2)' }}
              />
              {!mobile && <span style={{ color: 'var(--ol-ink-4)', fontSize: 12 }}>→</span>}
              <input
                value={ruleReplacementDraft}
                onChange={e => setRuleReplacementDraft(e.target.value)}
                placeholder={t('vocab.corrections.replacementPlaceholder')}
                style={{ height: 32, padding: '0 10px', border: '0.5px solid var(--ol-line-strong)', borderRadius: 8, background: 'var(--ol-surface-2)' }}
              />
              <Btn size="sm" variant="primary" onClick={() => void onAddCorrectionRule()} style={mobile ? { justifySelf: 'start' } : undefined}>{t('common.add')}</Btn>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, minHeight: correctionRules.length ? undefined : 20 }}>
              {correctionRules.length === 0 && (
                <span style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('vocab.corrections.empty')}</span>
              )}
              {correctionRules.map(rule => (
                <CorrectionRuleChip
                  key={rule.id}
                  rule={rule}
                  onToggle={() => void onToggleCorrectionRule(rule)}
                  onRemove={() => void onRemoveCorrectionRule(rule.id)}
                />
              ))}
            </div>
          </div>
        </Collapsible>
        <Collapsible
          embedded
          defaultOpen
          title={t('vocab.sectionTitle')}
          desc={t('vocab.tip')}
        >
          <div style={{ display: 'flex', gap: 8 }}>
            <input
              ref={inputRef}
              placeholder={t('vocab.placeholder')}
              onKeyDown={onKeyDown}
              style={{
                flex: 1, height: 36, padding: '0 12px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8, fontSize: 13,
                fontFamily: 'inherit', outline: 'none',
                background: 'var(--ol-surface-2)',
                transition: 'border-color 0.16s var(--ol-motion-quick), box-shadow 0.18s var(--ol-motion-soft), background 0.16s var(--ol-motion-quick)',
              }}
            />
            <Btn variant="primary" icon="plus" onClick={onAdd}>{t('common.add')}</Btn>
          </div>
          <div style={{ marginTop: 12, display: 'flex', flexWrap: 'wrap', gap: 8, minHeight: 80 }}>
            {loading && <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>}
            {!loading && error && (
              <div style={{ fontSize: 12, color: 'var(--ol-err)', lineHeight: 1.6 }}>
                {t('vocab.loadFailed', { err: error })}
              </div>
            )}
            {!loading && !error && entries.length === 0 && (
              <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>
                {t('vocab.empty')}
              </div>
            )}
            {!error && entries.map(e => (
              <VocabChip key={e.id} entry={e} onRemove={() => onRemove(e.id)} onToggle={() => onToggle(e)} />
            ))}
          </div>
        </Collapsible>
      </Card>
      <style>{`
        @keyframes ol-chip-in {
          from { opacity: 0; transform: scale(.92); filter: blur(5px); }
          to   { opacity: 1; transform: scale(1); filter: blur(0); }
        }
      `}</style>
    </>
  );
}

interface CorrectionRuleChipProps {
  rule: CorrectionRule;
  onToggle: () => void;
  onRemove: () => void;
}

function CorrectionRuleChip({ rule, onToggle, onRemove }: CorrectionRuleChipProps) {
  const { t } = useTranslation();
  const enabled = rule.enabled;
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '5px 8px 5px 10px',
        borderRadius: 999,
        border: '0.5px solid var(--ol-line-strong)',
        background: enabled ? 'var(--ol-surface)' : 'var(--ol-surface-2)',
        opacity: enabled ? 1 : 0.55,
        fontSize: 12,
        fontFamily: 'var(--ol-font-mono)',
      }}
    >
      <button
        onClick={onToggle}
        title={enabled ? t('vocab.corrections.tipDisabled') : t('vocab.corrections.tipEnabled')}
        style={{ background: 'transparent', border: 0, padding: 0, color: 'inherit', fontFamily: 'inherit', cursor: 'default' }}
      >
        {rule.pattern} → {rule.replacement}
      </button>
      <button
        onClick={onRemove}
        aria-label={t('vocab.corrections.removeAria')}
        style={{ width: 18, height: 18, borderRadius: 999, border: 0, background: 'rgba(0,0,0,0.06)', color: 'var(--ol-ink-4)', cursor: 'default' }}
      >
        ×
      </button>
    </span>
  );
}

interface VocabChipProps {
  entry: DictionaryEntry;
  onRemove: () => void;
  onToggle: () => void;
}

function VocabChip({ entry, onRemove, onToggle }: VocabChipProps) {
  const { t } = useTranslation();
  const enabled = entry.enabled;
  return (
    <span
      style={{
        // 父 flex 容器 minHeight: 80 会让 flex item 在 align-self 默认 stretch 下被拉到
        // 80px 高，chip borderRadius: 999 把高度变大渲染成"超大椭圆"。alignSelf:flex-start
        // 阻止拉伸，chip 始终保持 content 高度。
        alignSelf: 'flex-start',
        display: 'inline-flex', alignItems: 'center', gap: 6,
        padding: '5px 10px 5px 12px',
        borderRadius: 999,
        border: '0.5px solid var(--ol-line-strong)',
        background: enabled ? (entry.hits > 0 ? 'var(--ol-blue-soft)' : 'var(--ol-surface)') : 'var(--ol-surface-2)',
        opacity: enabled ? 1 : 0.55,
        fontSize: 12, color: 'var(--ol-ink)',
        fontFamily: 'var(--ol-font-mono)',
        transition: 'background 0.16s var(--ol-motion-quick), opacity 0.18s var(--ol-motion-soft), border-color 0.16s var(--ol-motion-quick)',
        animation: 'ol-chip-in 0.22s var(--ol-motion-spring)',
      }}
    >
      <button
        onClick={onToggle}
        title={enabled ? t('vocab.tipDisabled') : t('vocab.tipEnabled')}
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
        aria-label={t('vocab.removeAria')}
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
