import { useTranslation } from 'react-i18next';
import type { CorrectionRule } from '../../api/contracts';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { PageHeader } from '../../ui/layout/PageHeader';
import { useCorrectionRules } from './useCorrectionRules';

export function Corrections() {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const {
    busy,
    error,
    onlyLearnedRules,
    setOnlyLearnedRules,
    rulePatternDraft,
    setRulePatternDraft,
    ruleReplacementDraft,
    setRuleReplacementDraft,
    onAddCorrectionRule,
    onRemoveCorrectionRule,
    onToggleCorrectionRule,
    onRemoveAllLearnedRules,
    learnedRuleCount,
    visibleRules,
  } = useCorrectionRules();

  return (
    <>
      <PageHeader title={t('vocab.corrections.title')} desc={t('vocab.corrections.tip')} />

      <Card>
        <div style={{ display: 'grid', gap: 10 }}>
          <div
            className="ol-grid-12"
            data-columns={mobile ? '1' : undefined}
            style={{ alignItems: 'center' }}
          >
            <input
              className="ol-col-5"
              value={rulePatternDraft}
              onChange={(e) => setRulePatternDraft(e.target.value)}
              placeholder={t('vocab.corrections.patternPlaceholder')}
              style={{
                height: 32,
                padding: '0 10px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 'var(--ol-control-radius)',
                background: 'var(--ol-surface-2)',
                fontFamily: 'inherit',
                fontSize: 13,
              }}
            />
            {!mobile && (
              <span
                className="ol-col-1"
                aria-hidden
                style={{ color: 'var(--ol-ink-4)', fontSize: 12, textAlign: 'center' }}
              >
                →
              </span>
            )}
            <input
              className="ol-col-4"
              value={ruleReplacementDraft}
              onChange={(e) => setRuleReplacementDraft(e.target.value)}
              placeholder={t('vocab.corrections.replacementPlaceholder')}
              style={{
                height: 32,
                padding: '0 10px',
                border: '0.5px solid var(--ol-line-strong)',
                borderRadius: 'var(--ol-control-radius)',
                background: 'var(--ol-surface-2)',
                fontFamily: 'inherit',
                fontSize: 13,
              }}
            />
            <Btn
              size="sm"
              variant="primary"
              className="ol-correction-add ol-col-2"
              onClick={() => void onAddCorrectionRule()}
              disabled={busy}
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
              <Btn size="sm" disabled={busy} onClick={() => void onRemoveAllLearnedRules()}>
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
                onRemove={() => void onRemoveCorrectionRule(rule)}
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
        borderRadius: 'var(--ol-radius)',
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
            borderRadius: 'var(--ol-radius)',
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
          borderRadius: 'var(--ol-radius)',
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
