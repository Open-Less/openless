import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { GithubLoginModal } from '../../components/GithubLoginModal';
import { Modal } from '../../components/ui/Modal';
import { Btn, Card } from '../_atoms';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { marketplaceAuthStatus } from '../../lib/ipc';
import {
  cloudSyncDelete,
  cloudSyncRestore,
  cloudSyncStatus,
  cloudSyncUpload,
  type CloudSyncStatus,
} from '../../lib/ipc/cloud-sync';
import {
  getLocalePreference,
  setLocalePreference,
  SUPPORTED_LOCALES,
  type SupportedLocale,
} from '../../i18n';
import { readFontScale, setFontScale, type FontScaleId } from '../../lib/fontScale';

type Action = 'upload' | 'restore' | 'delete';

export function CloudSyncSection() {
  const { t, i18n } = useTranslation();
  const { prefs, updatePrefs, refresh } = useHotkeySettings();
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [status, setStatus] = useState<CloudSyncStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<Action | null>(null);
  const [message, setMessage] = useState('');
  const [failed, setFailed] = useState(false);
  const [showLogin, setShowLogin] = useState(false);
  const [confirm, setConfirm] = useState<'restore' | 'delete' | null>(null);
  const sequence = useRef(0);
  const login = prefs?.marketplaceDevLogin?.trim() ?? '';

  const showError = (error: unknown) => {
    const detail =
      error && typeof error === 'object'
        ? (error as {
            message?: string;
            code?: string;
            details?: { reason?: string; status?: number };
          })
        : null;
    const text = detail?.message ?? String(error);
    setFailed(true);
    if (detail?.details?.reason === 'revision_conflict') setMessage(t('cloudSync.conflict'));
    else if (
      detail?.code === 'unsupported' ||
      /cloud_sync_unavailable|404|not supported|unavailable/i.test(text)
    )
      setMessage(t('cloudSync.unavailable'));
    else if (detail?.code === 'permission_denied' || detail?.details?.status === 401) {
      setSignedIn(false);
      setStatus(null);
      setMessage(t('cloudSync.signInRequired'));
    } else setMessage(t('cloudSync.failed', { error: text.slice(0, 400) }));
  };

  const load = async () => {
    const request = ++sequence.current;
    setLoading(true);
    setStatus(null);
    setMessage('');
    try {
      const auth = await marketplaceAuthStatus();
      if (request !== sequence.current) return;
      setSignedIn(auth.signedIn);
      if (auth.signedIn) {
        const next = await cloudSyncStatus();
        if (request === sequence.current) setStatus(next);
      }
    } catch (error) {
      if (request === sequence.current) showError(error);
    } finally {
      if (request === sequence.current) setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    return () => {
      sequence.current += 1;
    };
  }, [login]);

  const perform = async (action: Action) => {
    if (!status || busy) return;
    sequence.current += 1;
    setConfirm(null);
    setBusy(action);
    setMessage('');
    try {
      if (action === 'upload') {
        setStatus(
          await cloudSyncUpload(status.revision, {
            locale: getLocalePreference(),
            fontScale: readFontScale(),
          }),
        );
      } else if (action === 'restore') {
        const restored = await cloudSyncRestore();
        setStatus(restored.status);
        await refresh();
        const locale = restored.uiPreferences.locale;
        if (locale === 'system' || SUPPORTED_LOCALES.some((item) => item === locale)) {
          await setLocalePreference(locale as SupportedLocale | 'system');
        }
        const scale = restored.uiPreferences.fontScale;
        if (scale === 'small' || scale === 'medium' || scale === 'large')
          setFontScale(scale as FontScaleId);
      } else {
        setStatus(await cloudSyncDelete(status.revision));
      }
      setFailed(false);
      setMessage(t(`cloudSync.${action}Success`));
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card>
      <section className="ol-cloud-sync" aria-labelledby="cloud-sync-title">
        <header>
          <span className="ol-cloud-sync-icon">
            <Icon name="cloud" size={21} />
          </span>
          <div>
            <h3 id="cloud-sync-title">{t('cloudSync.title')}</h3>
            <p>{t('cloudSync.description')}</p>
          </div>
          <button
            type="button"
            className="ol-settings-back"
            onClick={() => void load()}
            disabled={loading || busy !== null}
            aria-label={t('cloudSync.refresh')}
            title={t('cloudSync.refresh')}
          >
            <Icon name="refresh" size={16} />
          </button>
        </header>
        {loading ? (
          <p role="status">{t('cloudSync.loading')}</p>
        ) : signedIn === false ? (
          <Btn variant="primary" icon="user" onClick={() => setShowLogin(true)}>
            {t('cloudSync.signIn')}
          </Btn>
        ) : signedIn ? (
          <>
            <div className="ol-cloud-sync-account">
              <Icon name="user" size={16} />
              <span>{t('cloudSync.account')}</span>
              <strong dir="auto">{login ? `@${login}` : 'GitHub'}</strong>
            </div>
            {status && (
              <div className="ol-cloud-sync-backup">
                <strong>
                  {t(status.hasSnapshot ? 'cloudSync.available' : 'cloudSync.noBackup')}
                </strong>
                {status.hasSnapshot && <p>{t('cloudSync.summary', status.counts)}</p>}
                {status.updatedAt && (
                  <small>
                    {t('cloudSync.updated', {
                      time: new Date(status.updatedAt).toLocaleString(
                        i18n.resolvedLanguage ?? i18n.language,
                      ),
                    })}
                  </small>
                )}
              </div>
            )}
            <div className="ol-cloud-sync-actions">
              <Btn
                variant="primary"
                icon="upload"
                disabled={!status || busy !== null}
                onClick={() => void perform('upload')}
              >
                {t('cloudSync.upload')}
              </Btn>
              <Btn
                icon="download"
                disabled={!status?.hasSnapshot || busy !== null}
                onClick={() => setConfirm('restore')}
              >
                {t('cloudSync.restore')}
              </Btn>
              <button
                type="button"
                className="ol-vocab-delete-selected"
                disabled={!status?.hasSnapshot || busy !== null}
                onClick={() => setConfirm('delete')}
              >
                <Icon name="trash" size={14} />
                {t('cloudSync.delete')}
              </button>
            </div>
          </>
        ) : null}
        {busy && <p role="status">{t('cloudSync.working')}</p>}
        {message && (
          <p
            role={failed ? 'alert' : 'status'}
            className={failed ? 'ol-cloud-sync-error' : undefined}
          >
            {message}
          </p>
        )}
        <p className="ol-cloud-sync-scope">{t('cloudSync.scope')}</p>
      </section>
      {showLogin && (
        <GithubLoginModal
          onClose={() => setShowLogin(false)}
          onSuccess={(nextLogin) => {
            setShowLogin(false);
            void updatePrefs((current) => ({ ...current, marketplaceDevLogin: nextLogin }))
              .then(load)
              .catch(showError);
          }}
        />
      )}
      {confirm && (
        <CloudSyncConfirmation
          action={confirm}
          onCancel={() => setConfirm(null)}
          onConfirm={() => void perform(confirm)}
        />
      )}
    </Card>
  );
}

function CloudSyncConfirmation({
  action,
  onCancel,
  onConfirm,
}: {
  action: 'restore' | 'delete';
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation();
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef(document.activeElement);
  useEffect(() => {
    const previous = opener.current;
    const background =
      previous instanceof HTMLElement ? previous.closest<HTMLElement>('[role="dialog"]') : null;
    const wasInert = background?.inert ?? false;
    dialog.current?.querySelector('button')?.focus();
    if (background) background.inert = true;
    return () => {
      if (background) background.inert = wasInert;
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, []);
  return (
    <Modal zIndex={100} width="min(440px, 100%)" onClose={onCancel}>
      <div
        ref={dialog}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="cloud-confirm-title"
        aria-describedby="cloud-confirm-desc"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            onCancel();
          }
          if (event.key === 'Tab') {
            const buttons = dialog.current?.querySelectorAll('button');
            if (!buttons?.length) return;
            if (event.shiftKey && document.activeElement === buttons[0]) {
              event.preventDefault();
              buttons[buttons.length - 1].focus();
            } else if (!event.shiftKey && document.activeElement === buttons[buttons.length - 1]) {
              event.preventDefault();
              buttons[0].focus();
            }
          }
        }}
      >
        <h3 id="cloud-confirm-title" style={{ margin: 0 }}>
          {t(`cloudSync.${action}Title`)}
        </h3>
        <p id="cloud-confirm-desc" style={{ color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
          {t(`cloudSync.${action}Description`)}
        </p>
        <div className="ol-cloud-sync-actions">
          <Btn onClick={onCancel}>{t('common.cancel')}</Btn>
          <Btn variant="primary" onClick={onConfirm}>
            {t(action === 'restore' ? 'cloudSync.confirmRestore' : 'cloudSync.confirmDelete')}
          </Btn>
        </div>
      </div>
    </Modal>
  );
}
