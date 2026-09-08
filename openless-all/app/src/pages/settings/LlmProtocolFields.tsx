import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../components/ui/SelectLite';
import { readCredential, setCredential } from '../../lib/ipc';
import type { LlmRequestFormat } from '../../lib/ipc/providers';
import { emitSaved } from '../../lib/savedEvent';
import { SettingRow, inputStyle } from './shared';

const accounts = ['ark.request_format', 'ark.messages_thinking', 'ark.max_tokens', 'ark.thinking_budget'] as const;
type Account = typeof accounts[number];
export type ProtocolValues = Record<Account, string>;
const emptyValues: ProtocolValues = {
  'ark.request_format': '', 'ark.messages_thinking': '', 'ark.max_tokens': '', 'ark.thinking_budget': '',
};

/** 界面即时提示；Core 仍对实际存储和发出的请求做同样的校验。 */
export function protocolValidationError(values: ProtocolValues, defaultFormat: LlmRequestFormat): string | null {
  const format = values['ark.request_format'] || defaultFormat;
  if (!['chat_completions', 'responses', 'messages'].includes(format)) return 'llmRequestFormatInvalid';
  const mode = values['ark.messages_thinking'] || 'adaptive';
  if (!['adaptive', 'budget'].includes(mode)) return 'llmThinkingModeInvalid';
  const max = values['ark.max_tokens'] || '8192';
  const budget = values['ark.thinking_budget'] || '1024';
  if (![max, budget].every(value => /^\d+$/.test(value) && Number(value) > 0 && Number(value) <= 4294967295)) return 'llmTokenLimitInvalid';
  if (Number(budget) < 1024 || (format === 'messages' && mode === 'budget' && Number(budget) >= Number(max))) return 'llmThinkingBudgetInvalid';
  return null;
}

export function LlmProtocolFields({ channelId, defaultFormat, formats, onUserMutation, onBlockedChange, onSaved }: {
  channelId: string;
  defaultFormat: LlmRequestFormat;
  formats: LlmRequestFormat[];
  onUserMutation: () => void;
  onBlockedChange: (account: string, blocked: boolean) => void;
  onSaved?: () => void;
}) {
  const { t } = useTranslation();
  const [values, setValues] = useState<ProtocolValues>(emptyValues);
  const [saved, setSaved] = useState<ProtocolValues>(emptyValues);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<'read' | 'save' | null>(null);
  const mounted = useRef(true);
  const writing = useRef(false);
  const dirty = accounts.some(account => values[account] !== saved[account]);
  const validation = protocolValidationError(values, defaultFormat);

  useEffect(() => {
    mounted.current = true;
    Promise.all(accounts.map(account => readCredential(account, channelId)))
      .then(result => {
        if (!mounted.current) return;
        const next = Object.fromEntries(accounts.map((account, index) => [account, result[index] ?? ''])) as ProtocolValues;
        setValues(next); setSaved(next); setLoaded(true);
      })
      .catch(() => { if (mounted.current) setError('read'); });
    return () => { mounted.current = false; };
  }, [channelId]);

  useEffect(() => {
    onBlockedChange('protocol', !loaded || saving || dirty || error !== null || validation !== null);
  }, [loaded, saving, dirty, error, validation, onBlockedChange]);

  const save = async (next: ProtocolValues) => {
    if (!loaded || writing.current || protocolValidationError(next, defaultFormat)) return;
    writing.current = true;
    setSaving(true); setError(null);
    try {
      for (const account of accounts) {
        if (next[account] !== saved[account]) await setCredential(account, next[account], channelId);
      }
      if (mounted.current) {
        setSaved(next);
        emitSaved('saved', t('common.saved'));
        onSaved?.();
      }
    } catch {
      if (mounted.current) { setError('save'); emitSaved('failed', t('common.operationFailed')); }
    } finally {
      writing.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  const change = (account: Account, value: string, immediate = false) => {
    onUserMutation();
    const next = { ...values, [account]: value };
    setValues(next);
    if (immediate) void save(next);
  };
  const format = values['ark.request_format'] || defaultFormat;
  const mode = values['ark.messages_thinking'] || 'adaptive';
  const disabled = !loaded || saving;
  const numberField = (account: Account, label: string, placeholder: string) => (
    <SettingRow label={label}>
      <input type="number" min={account === 'ark.thinking_budget' ? 1024 : 1} step={1}
        aria-label={label} value={values[account]} placeholder={placeholder} disabled={disabled}
        onChange={event => change(account, event.target.value)} onBlur={() => { if (dirty) void save(values); }}
        style={{ ...inputStyle, width: '100%', maxWidth: 420 }} />
    </SettingRow>
  );
  return <>
    <SettingRow label={t('settings.providers.requestFormatLabel')}>
      <SelectLite value={format} disabled={disabled} ariaLabel={t('settings.providers.requestFormatLabel')}
        options={formats.map(value => ({ value, label: ({ chat_completions: 'Chat Completions', responses: 'Responses', messages: 'Messages' })[value] }))}
        onChange={value => change('ark.request_format', value, true)} />
    </SettingRow>
    {format === 'responses' && <p style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>{t('settings.providers.responsesThinkingHint')}</p>}
    {format === 'messages' && <>
      <SettingRow label={t('settings.providers.messagesThinkingLabel')}>
        <SelectLite value={mode} disabled={disabled} ariaLabel={t('settings.providers.messagesThinkingLabel')}
          options={[{ value: 'adaptive', label: t('settings.providers.thinkingAdaptive') }, { value: 'budget', label: t('settings.providers.thinkingBudget') }]}
          onChange={value => change('ark.messages_thinking', value, true)} />
      </SettingRow>
      {numberField('ark.max_tokens', t('settings.providers.maxTokensLabel'), '8192')}
      {mode === 'budget' && numberField('ark.thinking_budget', t('settings.providers.thinkingBudgetLabel'), '1024')}
      <p style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>{t('settings.providers.messagesThinkingHint')}</p>
    </>}
    {(validation || error) && <p role="alert" style={{ fontSize: 12, color: 'var(--ol-warn)' }}>
      {validation ? t(`settings.providers.${validation}`) : t(error === 'read' ? 'settings.providers.readFailed' : 'common.operationFailed')}
    </p>}
    {dirty && <button disabled={disabled || !!validation} onClick={() => void save(values)}>{t('settings.providers.saveProtocol')}</button>}
  </>;
}
