import { useContext, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../components/ui/SelectLite';
import { readCredential, setCredential } from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import {
  defaultRealtimeOptions,
  loadRealtimeOptions,
  realtimeOptionsAccount,
  saveRealtimeOptions,
  type RealtimeAuthMode,
  type RealtimeOptions,
} from '../../lib/realtimeOmni';
import { Btn } from '../_atoms';
import { ProviderFormContext } from './ProviderForm';
import { SettingRow, Toggle } from './shared';

/** Friendly fields for the typed options account, scoped to the displayed provider. */
export function RealtimeOmniOptionsFields({
  providerId,
  onAuthModeChange,
}: {
  providerId: string;
  onAuthModeChange?: (mode: RealtimeAuthMode) => void;
}) {
  const { t } = useTranslation();
  const form = useContext(ProviderFormContext);
  const [value, setValue] = useState<RealtimeOptions>({ ...defaultRealtimeOptions });
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(false);
  const mounted = useRef(true);
  const pending = useRef<Promise<boolean>>(Promise.resolve(true));
  const failed = useRef(false);
  const writing = useRef(false);
  const revision = useRef(0);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    setLoaded(false);
    setError(false);
    loadRealtimeOptions(providerId, readCredential)
      .then((next) => {
        if (cancelled) return;
        setValue(next);
        setLoaded(true);
        failed.current = false;
        pending.current = Promise.resolve(true);
        onAuthModeChange?.(next.authMode);
      })
      .catch(() => {
        if (!cancelled) {
          setError(true);
        }
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [providerId, reload, onAuthModeChange]);
  useEffect(() => {
    form?.track(realtimeOptionsAccount, !loaded || saving || error);
  }, [form?.track, loaded, saving, error]);
  useEffect(
    () =>
      form?.register(realtimeOptionsAccount, async () => {
        const ok = await pending.current;
        return ok && !failed.current;
      }),
    [form?.register],
  );

  const persist = (next: RealtimeOptions, clear = false) => {
    if (writing.current || form?.leaving) return;
    writing.current = true;
    form?.invalidate(realtimeOptionsAccount);
    const currentRevision = ++revision.current;
    setValue(next);
    onAuthModeChange?.(next.authMode);
    setSaving(true);
    setError(false);
    failed.current = false;
    emitSaved('saving', t('common.saving'));
    pending.current = (async () => {
      try {
        if (clear) await setCredential(realtimeOptionsAccount, '', providerId);
        else await saveRealtimeOptions(providerId, next, setCredential);
        if (mounted.current && currentRevision === revision.current) {
          setLoaded(true);
          emitSaved('saved', t('common.saved'));
        }
        return true;
      } catch {
        failed.current = true;
        if (mounted.current) {
          setError(true);
          emitSaved('failed', t('common.operationFailed'));
        }
        return false;
      } finally {
        writing.current = false;
        if (mounted.current && currentRevision === revision.current) setSaving(false);
      }
    })();
  };
  const disabled = !loaded || saving || form?.leaving;
  const prefix = 'settings.providers.realtimeOptions.';
  return (
    <div className="ol-omni-realtime-options">
      <SettingRow
        label={t(prefix + 'authLabel')}
        desc={t(
          prefix +
            (value.authMode === 'headers'
              ? 'headersHint'
              : value.authMode === 'api-key'
                ? 'apiKeyHint'
                : 'bearerHint'),
        )}
      >
        <SelectLite
          value={value.authMode}
          disabled={disabled}
          ariaLabel={t(prefix + 'authLabel')}
          options={(['bearer', 'api-key', 'headers'] as const).map((mode) => ({
            value: mode,
            label: t(prefix + { bearer: 'bearer', 'api-key': 'apiKey', headers: 'headers' }[mode]),
          }))}
          onChange={(authMode) => persist({ ...value, authMode: authMode as RealtimeAuthMode })}
        />
      </SettingRow>
      {providerId === 'openai-realtime' && (
        <>
          <SettingRow label={t(prefix + 'effortLabel')} desc={t(prefix + 'effortHint')}>
            <SelectLite
              value={value.reasoningEffort}
              disabled={disabled}
              ariaLabel={t(prefix + 'effortLabel')}
              options={[
                ['auto', 'auto'],
                ['omit', 'omit'],
                ['minimal', 'minimal'],
                ['low', 'low'],
                ['medium', 'medium'],
                ['high', 'high'],
                ['xhigh', 'xhigh'],
                ['none', 'none'],
              ].map(([level, label]) => ({ value: level, label: t(prefix + label) }))}
              onChange={(reasoningEffort) =>
                persist({
                  ...value,
                  reasoningEffort: reasoningEffort as RealtimeOptions['reasoningEffort'],
                })
              }
            />
          </SettingRow>
          {value.reasoningEffort === 'none' && (
            <SettingRow label={t(prefix + 'fallbackLabel')} desc={t(prefix + 'fallbackHint')}>
              <Toggle
                on={value.reasoningFallback}
                disabled={disabled}
                label={t(prefix + 'fallbackLabel')}
                onToggle={(reasoningFallback) => persist({ ...value, reasoningFallback })}
              />
            </SettingRow>
          )}
        </>
      )}
      {error && (
        <div role="alert" style={{ color: 'var(--ol-danger)', fontSize: 12 }}>
          {t(prefix + 'configError')}
          <Btn onClick={() => setReload((v) => v + 1)} disabled={saving}>
            {t(prefix + 'retry')}
          </Btn>
          <Btn onClick={() => persist({ ...defaultRealtimeOptions }, true)} disabled={saving}>
            {t(prefix + 'reset')}
          </Btn>
        </div>
      )}
    </div>
  );
}
