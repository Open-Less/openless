// Corrections.tsx — 「纠正规则」独立页。
// 修正常见 ASR 误识别：pattern → replacement，支持 {num} 一个数字通配。
// 自动收集（source === 'learned'）的规则可单独筛出并一键清空 —— 与词典页的
// 「自动添加」筛选同一套信任前提：用户随时能看清、能整块撤销。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  addCorrectionRule,
  isTauri,
  listCorrectionRules,
  removeCorrectionRule,
  setCorrectionRuleEnabled,
} from '../lib/ipc';
import type { CorrectionRule } from '../lib/types';
import { useMobileLayout } from '../lib/useMobileLayout';
import { Btn, Card, PageHeader } from './_atoms';

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

export function Corrections() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [rules, setRules] = useState<CorrectionRule[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [onlyLearnedRules, setOnlyLearnedRules] = useState(false);
  const [rulePatternDraft, setRulePatternDraft] = useState('');
  const [ruleReplacementDraft, setRuleReplacementDraft] = useState('');

  const refresh = async () => {
    try {
      const data = await listCorrectionRules();
      setRules(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    void refresh();
    if (!isTauri) return;
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    // A cloud restore updates vocabulary and correction stores together.
    void import('@tauri-apps/api/event')
      .then(async ({ listen }) => {
        const stop = await listen('vocab:updated', () => void refresh());
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((error) => console.warn('[corrections] update listener failed', error));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const onAddCorrectionRule = async () => {
    const pattern = rulePatternDraft.trim();
    if (!pattern) return;
    const replacement = ruleReplacementDraft.trim();
    if (!isSupportedCorrectionRule(pattern, replacement)) {
      setError(t('vocab.corrections.invalid'));
      return;
    }
    try {
      setError(null);
      const rule = await addCorrectionRule(pattern, replacement);
      setRules((prev) => [rule, ...prev]);
      setRulePatternDraft('');
      setRuleReplacementDraft('');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onRemoveCorrectionRule = async (id: string) => {
    try {
      await removeCorrectionRule(id);
      setRules((prev) => prev.filter((r) => r.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  const onRemoveAllLearnedRules = async () => {
    const learned = rules.filter((r) => r.source === 'learned');
    if (learned.length === 0) return;
    // 逐条删而不是加一个新的批量后端命令：规则数量是几十条量级，为此多开一条 IPC
    // 不值得，而且逐条删失败一条也不影响其余。
    const removed: string[] = [];
    for (const rule of learned) {
      try {
        await removeCorrectionRule(rule.id);
        removed.push(rule.id);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    }
    setRules((prev) => prev.filter((r) => !removed.includes(r.id)));
  };

  const learnedRuleCount = rules.filter((r) => r.source === 'learned').length;
  const visibleRules = onlyLearnedRules ? rules.filter((r) => r.source === 'learned') : rules;

  const onToggleCorrectionRule = async (rule: CorrectionRule) => {
    const next = !rule.enabled;
    setRules((prev) => prev.map((r) => (r.id === rule.id ? { ...r, enabled: next } : r)));
    try {
      await setCorrectionRuleEnabled(rule.id, next);
    } catch (err) {
      setRules((prev) => prev.map((r) => (r.id === rule.id ? { ...r, enabled: rule.enabled } : r)));
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <>
      <PageHeader
        title={t('vocab.corrections.title')}
        desc={t('vocab.corrections.tip')}
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm" onClick={() => void refresh()}>
              {t('common.refresh')}
            </Btn>
          </div>
        }
      />
      <Card>
        <div style={{ display: 'grid', gap: 10 }}>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: mobile ? '1fr' : 'minmax(0, 1fr) auto minmax(0, 1fr) auto',
              gap: 8,
              alignItems: mobile ? 'stretch' : 'center',
            }}
          >
            <input
              value={rulePatternDraft}
              onChange={(e) => setRulePatternDraft(e.target.value)}
              placeholder={t('vocab.corrections.patternPlaceholder')}
              style={{
                height: 32,
                padding: '0 10px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8,
                background: 'var(--ol-surface-2)',
                fontFamily: 'inherit',
                fontSize: 13,
              }}
            />
            {!mobile && <span style={{ color: 'var(--ol-ink-4)', fontSize: 12 }}>→</span>}
            <input
              value={ruleReplacementDraft}
              onChange={(e) => setRuleReplacementDraft(e.target.value)}
              placeholder={t('vocab.corrections.replacementPlaceholder')}
              style={{
                height: 32,
                padding: '0 10px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 8,
                background: 'var(--ol-surface-2)',
                fontFamily: 'inherit',
                fontSize: 13,
              }}
            />
            <Btn
              size="sm"
              variant="primary"
              onClick={() => void onAddCorrectionRule()}
              style={mobile ? { justifySelf: 'start' } : undefined}
            >
              {t('common.add')}
            </Btn>
          </div>
          {learnedRuleCount > 0 && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <label
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 6,
                  fontSize: 12,
                  color: 'var(--ol-ink-3)',
                }}
              >
                <input
                  type="checkbox"
                  checked={onlyLearnedRules}
                  onChange={(e) => setOnlyLearnedRules(e.target.checked)}
                />
                {t('vocab.corrections.onlyLearned', { count: learnedRuleCount })}
              </label>
              <Btn size="sm" onClick={() => void onRemoveAllLearnedRules()}>
                {t('vocab.corrections.removeAllLearned')}
              </Btn>
            </div>
          )}
          {error && (
            <div style={{ fontSize: 12, color: 'var(--ol-err)', lineHeight: 1.6 }}>{error}</div>
          )}
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              gap: 8,
              minHeight: visibleRules.length ? undefined : 20,
            }}
          >
            {visibleRules.length === 0 && (
              <span style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>
                {t('vocab.corrections.empty')}
              </span>
            )}
            {visibleRules.map((rule) => (
              <CorrectionRuleChip
                key={rule.id}
                rule={rule}
                onToggle={() => void onToggleCorrectionRule(rule)}
                onRemove={() => void onRemoveCorrectionRule(rule.id)}
              />
            ))}
          </div>
        </div>
      </Card>
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
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
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
        style={{
          background: 'transparent',
          border: 0,
          padding: 0,
          color: 'inherit',
          fontFamily: 'inherit',
          cursor: 'default',
        }}
      >
        {rule.pattern} → {rule.replacement}
      </button>
      {rule.source === 'learned' && (
        <span
          title={t('vocab.corrections.learnedTip')}
          style={{
            padding: '1px 5px',
            borderRadius: 4,
            fontSize: 10,
            background: 'var(--ol-blue-soft)',
            color: 'var(--ol-ink-3)',
            fontFamily: 'inherit',
            letterSpacing: 0.2,
          }}
        >
          {t('vocab.corrections.learnedBadge')}
        </span>
      )}
      <button
        onClick={onRemove}
        aria-label={t('vocab.corrections.removeAria')}
        style={{
          width: 18,
          height: 18,
          borderRadius: 999,
          border: 0,
          background: 'var(--ol-control-muted)',
          color: 'var(--ol-ink-4)',
          cursor: 'default',
        }}
      >
        ×
      </button>
    </span>
  );
}
