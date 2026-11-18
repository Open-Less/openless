import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { CorrectionRule } from '../../api/contracts';
import {
  addCorrectionRule,
  listCorrectionRules,
  removeCorrectionRule,
  setCorrectionRuleEnabled,
} from '../../api/corrections';
import { listenVocabularyChanges } from '../../api/vocabulary-events';
import { useInteraction } from '../../ui/overlays/InteractionProvider';

const numberToken = '{num}';
function supportedRule(pattern: string, replacement: string) {
  const [prefix, suffix, ...more] = pattern.split(numberToken);
  if (!pattern || more.length || (suffix === undefined && replacement.includes(numberToken)))
    return false;
  return suffix === undefined || Boolean(prefix || suffix);
}

const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function useCorrectionRules() {
  const { t } = useTranslation();
  const { confirm, notify } = useInteraction('corrections');
  const [rules, setRules] = useState<CorrectionRule[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [onlyLearnedRules, setOnlyLearnedRules] = useState(false);
  const [rulePatternDraft, setRulePatternDraft] = useState('');
  const [ruleReplacementDraft, setRuleReplacementDraft] = useState('');
  const writing = useRef(false);
  const request = useRef(0);
  const refresh = useCallback(
    async (withNotice = false) => {
      const id = ++request.current;
      try {
        const snapshot = await listCorrectionRules();
        if (id !== request.current) return;
        setRules(snapshot);
        if (withNotice) {
          setError(null);
          notify({ message: t('common.refreshed'), state: 'saved' });
        }
      } catch (error) {
        if (id !== request.current) return;
        setError(errorMessage(error));
        if (withNotice)
          notify({ message: t('common.operationFailed'), state: 'failed', durationMs: 3500 });
      }
    },
    [notify, t],
  );
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listenVocabularyChanges(() => {
      if (!disposed && !writing.current) void refreshRef.current();
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch((error) => console.warn('[corrections] update listener failed', error))
      .finally(() => {
        if (!disposed) void refreshRef.current();
      });
    return () => {
      disposed = true;
      request.current++;
      stop?.();
    };
  }, []);

  const write = async (command: () => Promise<void>, success = t('common.saved')) => {
    if (writing.current) return;
    writing.current = true;
    request.current++;
    setBusy(true);
    setError(null);
    try {
      await command();
      notify({ message: success, state: 'saved' });
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      writing.current = false;
      setBusy(false);
      await refresh();
    }
  };

  const onAddCorrectionRule = () => {
    const pattern = rulePatternDraft.trim();
    const replacement = ruleReplacementDraft.trim();
    if (!pattern) return;
    if (!supportedRule(pattern, replacement)) {
      setError(t('vocab.corrections.invalid'));
      return;
    }
    return write(async () => {
      await addCorrectionRule(pattern, replacement);
      setRulePatternDraft((current) => (current.trim() === pattern ? '' : current));
      setRuleReplacementDraft((current) => (current.trim() === replacement ? '' : current));
    });
  };

  const onRemoveCorrectionRule = (id: string) => write(() => removeCorrectionRule(id));

  const onToggleCorrectionRule = (rule: CorrectionRule) =>
    write(() => setCorrectionRuleEnabled(rule.id, !rule.enabled));

  const learned = rules.filter((rule) => rule.source === 'learned');
  const onRemoveAllLearnedRules = async () => {
    if (!learned.length || writing.current) return;
    if (
      !(await confirm({
        title: t('vocab.corrections.removeAllLearned'),
        description: t('vocab.corrections.onlyLearned', { count: learned.length }),
        confirmLabel: t('common.delete'),
        tone: 'danger',
      }))
    )
      return;
    await write(async () => {
      const errors: unknown[] = [];
      for (const { id } of learned) {
        try {
          await removeCorrectionRule(id);
        } catch (error) {
          errors.push(error);
        }
      }
      if (errors.length) throw errors[0];
    });
  };
  return {
    busy,
    error,
    refresh,
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
    learnedRuleCount: learned.length,
    visibleRules: onlyLearnedRules ? learned : rules,
  };
}
