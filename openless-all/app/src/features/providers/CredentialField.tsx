import {
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type MutableRefObject,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { copyTextToClipboard } from '../../api/files';
import { readCredential, setCredential } from '../../api/providers';
import { Icon } from '../../components/Icon';
import { emitSaved } from '../../lib/savedEvent';
import { dismissNotification } from '../../lib/notifications';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { SelectLite, type SelectOption } from '../../ui/controls/SelectLite';
import {
  ChannelFormRow,
  CUSTOM_MODEL_OPTION_VALUE,
  iconBtnStyle,
  matchesEndpointPreset,
} from './channelFieldsUi';
import { CredentialDraft } from './credentialDraft';
import { ProviderFormContext } from './ProviderForm';

interface CredentialFieldProps {
  onBlockedChange?: (account: string, blocked: boolean) => void;
  label: string;
  account: string;
  provider?: string;
  placeholder?: string;
  pickerSaveRef?: MutableRefObject<((value: string) => Promise<boolean>) | null>;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
  trailing?: ReactNode;
  onValueChange?: (value: string) => void;
  /** Fires only when the user directly edits the field; initial read, copy, and show/hide do not trigger it. */
  onUserMutation?: () => void;
  /** When provided, renders a dropdown (preset selection) instead of an input; appends the current value as a custom item if not in the presets. */
  options?: SelectOption[];
  /** Endpoint presets and manual input share one field and one save queue. */
  endpointPresets?: { label: string; options: SelectOption[] };
}

export function CredentialField({
  label,
  account,
  provider,
  placeholder,
  pickerSaveRef,
  mono,
  mask,
  defaultValue,
  trailing,
  onValueChange,
  onUserMutation,
  options,
  endpointPresets,
  onBlockedChange,
}: CredentialFieldProps) {
  const fieldId = useId();
  const { t } = useTranslation();
  const form = useContext(ProviderFormContext);
  const draft = useMemo(
    () =>
      new CredentialDraft(
        () => readCredential(account, provider),
        (next) => setCredential(account, next, provider),
      ),
    [account, provider],
  );
  const { value, loaded, dirty, status } = useSyncExternalStore(draft.subscribe, draft.snapshot);
  const [revealed, setRevealed] = useState(false);
  const [customModelMode, setCustomModelMode] = useState(false);
  const onValueChangeRef = useRef(onValueChange);
  onValueChangeRef.current = onValueChange;
  const track = form?.track;
  const register = form?.register;

  useEffect(() => {
    setRevealed(false);
    setCustomModelMode(false);
    void draft.load();
    return () => draft.dispose();
  }, [draft]);
  useEffect(() => onValueChangeRef.current?.(draft.snapshot().value), [draft, loaded]);
  useEffect(() => {
    if (!pickerSaveRef) return;
    // A fetched model uses the same draft and queue as a manually typed model.
    const save = (next: string) => {
      const { loaded, dirty } = draft.snapshot();
      if (!loaded || dirty) return Promise.resolve(false);
      draft.edit(next);
      onValueChangeRef.current?.(next);
      return draft.flush();
    };
    pickerSaveRef.current = save;
    return () => {
      if (pickerSaveRef.current === save) pickerSaveRef.current = null;
    };
  }, [draft, pickerSaveRef]);
  useLayoutEffect(() => register?.(account, draft.flush), [draft, account, register]);
  useEffect(() => {
    const blocked = !loaded || dirty;
    track?.(account, blocked);
    onBlockedChange?.(account, blocked);
  }, [track, onBlockedChange, account, loaded, dirty]);
  useEffect(() => {
    const notice = `credential:${fieldId}`;
    if (status === 'saving') {
      emitSaved('saving', t('common.saving'), notice);
      return () => dismissNotification(notice);
    }
    if (status === 'saved')
      emitSaved(
        'saved',
        t(account.endsWith('api_key') ? 'common.apiKeySaved' : 'common.saved'),
        notice,
      );
    if (status === 'saveError') emitSaved('failed', t('common.operationFailed'), notice);
  }, [account, fieldId, status, t]);

  const edit = (next: string, immediate = false) => {
    if (!loaded || form?.leaving) return;
    onUserMutation?.();
    draft.edit(next);
    onValueChangeRef.current?.(next);
    if (immediate) void draft.flush();
    else draft.schedule();
  };
  const onCopy = async () => {
    if (!value || !loaded) return;
    try {
      await copyTextToClipboard(value);
      emitSaved('saved', t('common.copied'));
    } catch {
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const inputType = mask && !revealed ? 'password' : 'text';
  const disabled = !loaded || form?.leaving;
  const showInsecureEndpointWarning =
    (account === 'ark.endpoint' || account === 'asr.endpoint' || account === 'omni.endpoint') &&
    value.trim().toLowerCase().startsWith('http://');

  const presetValue =
    endpointPresets?.options.find((option) => {
      return matchesEndpointPreset(value || defaultValue || '', option.value);
    })?.value || '';

  return (
    <>
      {endpointPresets && (
        <ChannelFormRow label={endpointPresets.label}>
          <SelectLite
            value={presetValue}
            options={endpointPresets.options}
            placeholder={loaded ? t('settings.providers.presets.custom') : t('common.loading')}
            disabled={disabled || status === 'readError'}
            ariaLabel={endpointPresets.label}
            onChange={(next) => edit(next, true)}
            style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
          />
        </ChannelFormRow>
      )}
      <ChannelFormRow label={label} htmlFor={options && !customModelMode ? undefined : fieldId}>
        <div
          style={{ display: 'flex', flexDirection: 'column', gap: 5, width: '100%', minWidth: 0 }}
        >
          <div
            style={{
              display: 'flex',
              gap: 6,
              alignItems: 'center',
              width: '100%',
              flexWrap: 'nowrap',
            }}
          >
            {options && !customModelMode ? (
              <SelectLite
                value={value}
                onChange={(v) => {
                  // "Custom model…" escape hatch: switch back to the input to type any model name.
                  if (v === CUSTOM_MODEL_OPTION_VALUE) {
                    setCustomModelMode(true);
                    return;
                  }
                  edit(v, true);
                }}
                options={[
                  ...(value && !options.some((o) => o.value === value)
                    ? [{ value, label: value }]
                    : []),
                  ...options,
                  {
                    value: CUSTOM_MODEL_OPTION_VALUE,
                    label: t('settings.providers.customModelLabel', 'Custom model…'),
                  },
                ]}
                placeholder={loaded ? placeholder : t('common.loading')}
                disabled={disabled}
                ariaLabel={label}
                style={{
                  flex: 1,
                  height: 38,
                  minWidth: 0,
                  maxWidth: '100%',
                  fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit',
                }}
              />
            ) : (
              <input
                id={fieldId}
                type={inputType}
                value={value}
                placeholder={loaded ? placeholder : t('common.loading')}
                onChange={(event) => edit(event.target.value)}
                onBlur={() => void draft.flush()}
                disabled={disabled}
                readOnly={!!endpointPresets && !!presetValue}
                style={{
                  ...inputStyle,
                  flex: 1,
                  height: 38,
                  minWidth: 0,
                  maxWidth: '100%',
                  fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit',
                }}
              />
            )}
            {options && customModelMode && (
              <button
                onClick={() => setCustomModelMode(false)}
                title={t('settings.providers.presetListLabel', 'Back to presets')}
                style={iconBtnStyle}
                disabled={disabled}
              >
                <Icon name="chevDown" size={13} />
              </button>
            )}
            {defaultValue && !value && loaded && (
              <button
                onClick={() => edit(defaultValue, true)}
                title={t('settings.providers.fillDefault')}
                style={iconBtnStyle}
                disabled={!loaded}
              >
                <Icon name="check" size={13} />
              </button>
            )}
            {mask && (
              <button
                onClick={() => setRevealed((r) => !r)}
                title={revealed ? t('common.hide') : t('common.show')}
                style={iconBtnStyle}
                disabled={disabled}
              >
                <Icon name="eye" size={14} />
              </button>
            )}
            <button
              onClick={onCopy}
              title={t('common.copy')}
              style={iconBtnStyle}
              disabled={!value || disabled}
            >
              <Icon name="copy" size={14} />
            </button>
            {/* readError is a persistent "field unreadable" error and stays in place to tell the
              user the field is unavailable; other transient states (saving / saved / saveError /
              copied / copyError) go through emitSaved to the window notification host instead of
              inline placeholders. */}
            {status === 'readError' && (
              <span
                style={{
                  fontSize: 11,
                  color: 'var(--ol-warn)',
                  whiteSpace: 'nowrap',
                }}
              >
                {t('settings.providers.readFailed')}
              </span>
            )}
          </div>
          {trailing && (
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 6 }}>
              {trailing}
            </div>
          )}
          {showInsecureEndpointWarning && (
            <span style={{ fontSize: 11, color: 'var(--ol-warn)', lineHeight: 1.45 }}>
              {t('settings.providers.endpointHttpWarning')}
            </span>
          )}
        </div>
      </ChannelFormRow>
    </>
  );
}
