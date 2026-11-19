import { useContext, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../components/ui/SelectLite';
import { readCredential, setCredential } from '../../api/providers';
import type { LlmRequestFormat } from '../../api/providers';
import { emitSaved } from '../../lib/savedEvent';
import { Button as Btn } from '../../ui/controls/Button';
import { SettingRow, Toggle } from '../../ui/controls/SettingRow';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { ProviderFormContext } from './ProviderForm';

import {
  emptyValues,
  protocolAccounts,
  protocolValidationError,
  serviceTierAccount,
  type Account,
  type ProtocolValues,
} from './llmProtocol';
export { protocolValidationError, type ProtocolValues } from './llmProtocol';

export function LlmProtocolFields({
  channelId,
  providerType,
  defaultFormat,
  formats,
  onUserMutation,
  onBlockedChange,
  onSaved,
}: {
  channelId: string;
  providerType: string;
  defaultFormat: LlmRequestFormat;
  formats: LlmRequestFormat[];
  onUserMutation: () => void;
  onBlockedChange: (account: string, blocked: boolean) => void;
  onSaved?: (changedAccounts?: string[]) => void;
}) {
  const { t } = useTranslation();
  const form = useContext(ProviderFormContext);
  const [values, setValues] = useState<ProtocolValues>(emptyValues);
  const [saved, setSaved] = useState<ProtocolValues>(emptyValues);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<'read' | 'save' | null>(null);
  const arkServiceTier = providerType === 'ark';
  const accounts = arkServiceTier ? [...protocolAccounts, serviceTierAccount] : protocolAccounts;
  const mounted = useRef(true);
  const writing = useRef(false);
  const pendingWrite = useRef<Promise<boolean>>(Promise.resolve(true));
  const flushRef = useRef<() => Promise<boolean>>(() => Promise.resolve(true));
  const dirty = accounts.some((account) => values[account] !== saved[account]);
  const validation = protocolValidationError(values, defaultFormat);

  useEffect(() => {
    mounted.current = true;
    Promise.all(accounts.map((account) => readCredential(account, channelId)))
      .then((result) => {
        if (!mounted.current) return;
        const next = {
          ...emptyValues,
          ...Object.fromEntries(accounts.map((account, index) => [account, result[index] ?? ''])),
        } as ProtocolValues;
        setValues(next);
        setSaved(next);
        setLoaded(true);
      })
      .catch(() => {
        if (mounted.current) setError('read');
      });
    return () => {
      mounted.current = false;
    };
  }, [channelId, providerType]);

  useEffect(() => {
    onBlockedChange(
      'protocol',
      !loaded || saving || dirty || error !== null || validation !== null,
    );
  }, [loaded, saving, dirty, error, validation, onBlockedChange]);
  // The host Modal converges unwritten changes before close/switch; flush always reads the latest dirty state.
  useEffect(() => form?.register('protocol', () => flushRef.current()), [form?.register]);

  const save = (next: ProtocolValues): Promise<boolean> => {
    if (writing.current) return pendingWrite.current;
    if (!loaded || protocolValidationError(next, defaultFormat)) return Promise.resolve(false);
    writing.current = true;
    setSaving(true);
    setError(null);
    pendingWrite.current = (async () => {
      try {
        const changedAccounts = accounts.filter((account) => next[account] !== saved[account]);
        for (const account of changedAccounts) {
          await setCredential(account, next[account], channelId);
        }
        if (mounted.current) {
          setSaved(next);
          emitSaved('saved', t('common.saved'));
          onSaved?.(changedAccounts);
        }
        return true;
      } catch {
        if (mounted.current) {
          setError('save');
          emitSaved('failed', t('common.operationFailed'));
        }
        return false;
      } finally {
        writing.current = false;
        if (mounted.current) setSaving(false);
      }
    })();
    return pendingWrite.current;
  };
  flushRef.current = () =>
    writing.current ? pendingWrite.current : dirty ? save(values) : Promise.resolve(true);

  const change = (account: Account, value: string, immediate = false) => {
    if (!loaded || writing.current || form?.leaving) return;
    onUserMutation();
    const next = { ...values, [account]: value };
    setValues(next);
    if (immediate) void save(next);
  };
  const format = values['ark.request_format'] || defaultFormat;
  const mode = values['ark.messages_thinking'] || 'adaptive';
  const disabled = !loaded || saving || form?.leaving;
  const numberField = (account: Account, label: string, placeholder: string) => (
    <SettingRow label={label}>
      <input
        type="number"
        min={account === 'ark.thinking_budget' ? 1024 : 1}
        step={1}
        aria-label={label}
        value={values[account]}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => change(account, event.target.value)}
        onBlur={() => {
          if (dirty) void save(values);
        }}
        style={{ ...inputStyle, width: '100%', maxWidth: 420 }}
      />
    </SettingRow>
  );
  return (
    <>
      <SettingRow label={t('settings.providers.requestFormatLabel')}>
        <SelectLite
          value={format}
          disabled={disabled}
          ariaLabel={t('settings.providers.requestFormatLabel')}
          options={formats.map((value) => ({
            value,
            label: {
              chat_completions: 'Chat Completions',
              responses: 'Responses',
              messages: 'Messages',
            }[value],
          }))}
          onChange={(value) => change('ark.request_format', value, true)}
        />
      </SettingRow>
      {format === 'responses' && (
        <p style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
          {t('settings.providers.responsesThinkingHint')}
        </p>
      )}
      {arkServiceTier && (
        <SettingRow
          label={t('settings.providers.serviceTierLabel')}
          desc={t('settings.providers.serviceTierHint')}
        >
          <Toggle
            on={values['ark.service_tier'] === 'fast'}
            disabled={disabled}
            label={t('settings.providers.serviceTierLabel')}
            onToggle={(enabled) => change('ark.service_tier', enabled ? 'fast' : '', true)}
          />
        </SettingRow>
      )}
      {format === 'messages' && (
        <>
          <SettingRow label={t('settings.providers.messagesThinkingLabel')}>
            <SelectLite
              value={mode}
              disabled={disabled}
              ariaLabel={t('settings.providers.messagesThinkingLabel')}
              options={[
                { value: 'adaptive', label: t('settings.providers.thinkingAdaptive') },
                { value: 'budget', label: t('settings.providers.thinkingBudget') },
              ]}
              onChange={(value) => change('ark.messages_thinking', value, true)}
            />
          </SettingRow>
          {numberField('ark.max_tokens', t('settings.providers.maxTokensLabel'), '8192')}
          {mode === 'budget' &&
            numberField('ark.thinking_budget', t('settings.providers.thinkingBudgetLabel'), '1024')}
          <p style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
            {t('settings.providers.messagesThinkingHint')}
          </p>
        </>
      )}
      {(validation || error) && (
        <p role="alert" style={{ fontSize: 12, color: 'var(--ol-warn)' }}>
          {validation
            ? t(`settings.providers.${validation}`)
            : t(error === 'read' ? 'settings.providers.readFailed' : 'common.operationFailed')}
        </p>
      )}
      {dirty && (
        <Btn size="sm" disabled={disabled || !!validation} onClick={() => void save(values)}>
          {t('settings.providers.saveProtocol')}
        </Btn>
      )}
    </>
  );
}
