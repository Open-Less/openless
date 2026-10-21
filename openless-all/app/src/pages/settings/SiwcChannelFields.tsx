import { useCallback, useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '../../components/ui/Modal';
import { SelectLite } from '../../components/ui/SelectLite';
import { Btn } from '../_atoms';
import {
  siwcAction,
  siwcStatus,
  siwcModels,
  siwcErrorCode,
  readCredential,
  setCredential,
  openExternal,
  validateProviderCredentials,
  recordChannelTest,
  type SiwcAction,
  type SiwcStatus,
  type SiwcModel,
} from '../../lib/ipc';

const USAGE_URL = 'https://chatgpt.com/settings/usage';
export function SiwcChannelFields({
  channelId,
  onUserMutation,
  onTested,
}: {
  channelId: string;
  onUserMutation?: () => void;
  onTested?: () => void;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<SiwcStatus | null>(null);
  const [models, setModels] = useState<SiwcModel[]>([]);
  const [model, setModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [message, setMessage] = useState('');
  const [notice, setNotice] = useState('');
  const alive = useRef(true);
  const catalogRequest = useRef(0);
  const titleId = useId();
  const explain = useCallback(
    (error: unknown) => {
      const code = siwcErrorCode(error);
      if (code === 'remote_revocation_unconfirmed') return t('siwc.remoteUnconfirmed');
      return (
        t('siwc.errors.' + code, { defaultValue: t('siwc.failed') }) +
        (code === 'request_failed' ? '' : ' (' + code + ')')
      );
    },
    [t],
  );
  const refresh = useCallback(async () => {
    try {
      const next = await siwcStatus(channelId);
      if (alive.current) setStatus(next);
    } catch (error) {
      if (alive.current) setMessage(explain(error));
    }
  }, [channelId, explain]);
  useEffect(() => {
    alive.current = true;
    void refresh();
    const focus = () => void refresh();
    window.addEventListener('focus', focus);
    return () => {
      alive.current = false;
      catalogRequest.current++;
      window.removeEventListener('focus', focus);
    };
  }, [refresh]);
  useEffect(() => {
    if (status?.state !== 'authorizing') return;
    const timer = window.setInterval(() => void refresh(), 1000);
    return () => window.clearInterval(timer);
  }, [status?.state, refresh]);
  const loadModels = useCallback(async () => {
    const request = ++catalogRequest.current;
    setLoadingModels(true);
    setMessage('');
    try {
      const [catalog, saved] = await Promise.all([
        siwcModels(channelId),
        readCredential('ark.model_id', channelId),
      ]);
      if (!alive.current || request !== catalogRequest.current) return;
      setModels(catalog);
      setModel(saved ?? '');
    } catch (error) {
      if (alive.current && request === catalogRequest.current) {
        setModels([]);
        setMessage(explain(error));
      }
    } finally {
      if (alive.current && request === catalogRequest.current) setLoadingModels(false);
    }
  }, [channelId, explain]);
  const connected = status?.state === 'connected';
  useEffect(() => {
    if (connected) void loadModels();
    else {
      catalogRequest.current++;
      setModels([]);
      setLoadingModels(false);
    }
  }, [connected, status?.account?.id, loadModels]);
  const action = async (action: SiwcAction, registrationId?: string) => {
    onUserMutation?.();
    setBusy(true);
    setMessage('');
    setNotice('');
    try {
      const result = await siwcAction(channelId, action, registrationId);
      if (!alive.current) return;
      if (result?.remoteRevoked === false) setNotice(t('siwc.remoteUnconfirmed'));
      await refresh();
      onTested?.();
    } catch (error) {
      if (alive.current) setMessage(explain(error));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  const chooseModel = async (value: string) => {
    onUserMutation?.();
    setBusy(true);
    setMessage('');
    try {
      await setCredential('ark.model_id', value, channelId);
      if (alive.current) {
        setModel(value);
        onTested?.();
      }
    } catch (error) {
      if (alive.current) setMessage(explain(error));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  const verify = async () => {
    setBusy(true);
    setMessage('');
    const started = performance.now();
    try {
      await validateProviderCredentials('llm', channelId);
      await recordChannelTest(
        'llm',
        channelId,
        true,
        Math.round(performance.now() - started),
        null,
      );
      if (alive.current) setMessage(t('siwc.testSuccess'));
    } catch (error) {
      if (alive.current) setMessage(explain(error));
      await recordChannelTest(
        'llm',
        channelId,
        false,
        null,
        'siwc:' + JSON.stringify({ code: siwcErrorCode(error) }),
      ).catch(() => {});
    } finally {
      if (alive.current) {
        setBusy(false);
        await refresh();
        onTested?.();
      }
    }
  };
  if (!status) return <p role="status">{message || t('common.loading')}</p>;
  const authorizing = status.state === 'authorizing';
  const invalidModel = !!model && !models.some((m) => m.slug === model);
  return (
    <section
      aria-label="ChatGPT Plan"
      style={{ display: 'grid', gap: 16, fontSize: 14, lineHeight: 1.6 }}
    >
      <p style={{ margin: 0, color: 'var(--ol-ink-3)' }}>{t('siwc.description')}</p>
      {status.accounts.length > 0 && (
        <SelectLite
          value={status.account?.id ?? ''}
          options={status.accounts.map((account) => ({ value: account.id, label: account.label }))}
          onChange={(id) => void action('select', id)}
          ariaLabel={t('siwc.account')}
          disabled={busy || authorizing}
        />
      )}
      <div role="status">
        <strong>{t('siwc.states.' + status.state)}</strong>
      </div>
      <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
        {authorizing ? (
          <Btn disabled={busy} onClick={() => void action('cancel')}>
            {t('siwc.cancel')}
          </Btn>
        ) : (
          <>
            <Btn
              variant={connected || status.state === 'usage_limited' ? 'ghost' : 'primary'}
              disabled={busy}
              onClick={() =>
                void action(status.state === 'usage_disabled' ? 'enable_usage' : 'sign_in')
              }
            >
              {status.account
                ? t(status.state === 'usage_disabled' ? 'siwc.enableUsage' : 'siwc.reauthorize')
                : 'Continue with ChatGPT'}
            </Btn>
            {status.accounts.length > 0 && (
              <Btn disabled={busy} onClick={() => void action('add_account')}>
                {t('siwc.addAccount')}
              </Btn>
            )}
            {status.account && status.state !== 'reauthorization_required' && (
              <Btn disabled={busy} onClick={() => void action('disconnect')}>
                {t('siwc.disconnect')}
              </Btn>
            )}
          </>
        )}
        <Btn
          variant={status.state === 'usage_limited' ? 'primary' : 'ghost'}
          onClick={() => void openExternal(USAGE_URL)}
        >
          {t('siwc.manageUsage')}
        </Btn>
        {status.state === 'usage_limited' && (
          <Btn disabled={busy} onClick={() => void action('retry')}>
            {t('siwc.retry')}
          </Btn>
        )}
      </div>
      {status.error && (
        <p role="alert" style={{ margin: 0, color: 'var(--ol-warn)' }}>
          {explain(status.error)}
        </p>
      )}
      {connected && (
        <>
          <label>{t('siwc.model')}</label>
          <SelectLite
            value={invalidModel ? '' : model}
            options={models.map((m) => ({ value: m.slug, label: m.displayName }))}
            onChange={(value) => void chooseModel(value)}
            ariaLabel={t('siwc.model')}
            placeholder={t(loadingModels ? 'common.loading' : 'siwc.selectModel')}
            searchable
            searchPlaceholder={t('settings.providers.searchModels')}
            disabled={busy || loadingModels || models.length === 0}
          />
          {invalidModel && (
            <p role="alert" style={{ margin: 0, color: 'var(--ol-warn)' }}>
              {t('siwc.modelUnavailable', { model })}
            </p>
          )}
          {!loadingModels && models.length === 0 && (
            <p style={{ margin: 0 }}>{t('siwc.noModels')}</p>
          )}
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            <Btn disabled={busy || loadingModels} onClick={() => void loadModels()}>
              {t('siwc.refreshModels')}
            </Btn>
            <Btn
              disabled={busy || loadingModels || !model || invalidModel}
              onClick={() => void verify()}
            >
              {t('siwc.test')}
            </Btn>
          </div>
          <p style={{ margin: 0, color: 'var(--ol-ink-3)' }}>{t('siwc.testHint')}</p>
        </>
      )}
      {message && (
        <p role="status" style={{ margin: 0 }}>
          {message}
        </p>
      )}
      {notice && status.error?.code !== 'remote_revocation_unconfirmed' && (
        <p role="alert" style={{ margin: 0, color: 'var(--ol-warn)' }}>
          {notice}
        </p>
      )}
      {status.welcomePending && (
        <Modal labelledBy={titleId} zIndex={100} onClose={() => void action('acknowledge')}>
          <h2 id={titleId}>{t('siwc.welcome')}</h2>
          <p>{t('siwc.description')}</p>
          <Btn variant="primary" disabled={busy} onClick={() => void action('acknowledge')}>
            {t('siwc.gotIt')}
          </Btn>
        </Modal>
      )}
    </section>
  );
}
