import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CloudIcon, XIcon } from 'lucide-react';
import { isTauriHost } from '../../runtime/environment';
import { getCloudSyncPreviewScenario } from '../../preview/cloud-sync';
import {
  cloudSyncE2eeClaimSetupPrompt,
  cloudSyncE2eeStatus,
  listenCloudSyncEligibilityChanges,
  encryptedSyncScope,
  encryptedSyncErrorKey,
  CLOUD_SYNC_E2EE_CONSENT_VERSION,
  type EncryptedSyncStatus,
} from '../../api/cloud-sync';
import { CloudSyncSection, CloudSyncTermsDialog, prepareEncryptedSync } from './CloudSyncSection';
import { PresenceModal } from '../../ui/overlays/Modal';

/** Core owns eligibility and the durable once-per-installation claim. */
export function CloudSyncSetupPrompt({
  blocked,
  onSetup,
}: {
  blocked: boolean;
  onSetup: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [review, setReview] = useState<EncryptedSyncStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const busyRef = useRef(false);
  const dismissed = useRef<EncryptedSyncStatus | null>(null);
  const reviewRef = useRef(review);
  reviewRef.current = review;
  const checkRef = useRef(0);
  const focusRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const native = isTauriHost();
    const previewReview = getCloudSyncPreviewScenario() === 'reconsent';
    if ((!native && !previewReview) || blocked) {
      setOpen(false);
      setReview(null);
      checkRef.current += 1;
      return;
    }
    let cancelled = false;
    let inFlight = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: (() => void) | undefined;
    const foreground = () =>
      previewReview || (document.visibilityState === 'visible' && document.hasFocus());
    const check = async () => {
      if (
        cancelled ||
        inFlight ||
        busyRef.current ||
        !foreground()
      )
        return;
      inFlight = true;
      const request = ++checkRef.current;
      try {
        const status = await cloudSyncE2eeStatus();
        if (cancelled || checkRef.current !== request) return;
        if (status.enabled && status.consentVersion !== CLOUD_SYNC_E2EE_CONSENT_VERSION) {
          const scope = encryptedSyncScope(status);
          const prior = dismissed.current;
          const remainsDismissed = Boolean(
            prior &&
              prior.serviceOrigin === status.serviceOrigin &&
              (!prior.account || prior.account.githubId === status.account?.githubId) &&
              (!prior.vaultId || prior.vaultId === status.vaultId),
          );
          if (remainsDismissed) dismissed.current = status;
          if (foreground() && !remainsDismissed) {
            setOpen(false);
            if (reviewRef.current && encryptedSyncScope(reviewRef.current) !== scope)
              setError(undefined);
            setReview(status);
          } else {
            setReview(null);
          }
          return;
        }
        setReview(null);
        if (previewReview) return;
        const claimed = await cloudSyncE2eeClaimSetupPrompt();
        if (!cancelled && checkRef.current === request && claimed && foreground())
          setOpen(true);
      } catch {
        // Denied credential reads and in-progress saves never consume consent
        // or display a misleading configuration-complete message.
      } finally {
        inFlight = false;
      }
    };
    const schedule = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        void check();
      }, 300);
    };
    const invalidate = () => {
      if (busyRef.current) return;
      checkRef.current += 1;
      setOpen(false);
      setReview(null);
    };
    const eligibilityChanged = () => {
      // Recheck a displayed review without resetting its acknowledgement.
      if (!reviewRef.current || document.visibilityState !== 'visible') invalidate();
      schedule();
    };
    if (previewReview) {
      schedule();
      return () => {
        cancelled = true;
        checkRef.current += 1;
        if (timer) clearTimeout(timer);
      };
    }
    void listenCloudSyncEligibilityChanges(eligibilityChanged)
      .then((handle) => {
        if (cancelled) handle();
        else {
          unlisten = handle;
          schedule();
        }
      })
      .catch(() => {});
    window.addEventListener('focus', schedule);
    window.addEventListener('blur', invalidate);
    document.addEventListener('visibilitychange', eligibilityChanged);
    return () => {
      cancelled = true;
      checkRef.current += 1;
      if (timer) clearTimeout(timer);
      unlisten?.();
      window.removeEventListener('focus', schedule);
      window.removeEventListener('blur', invalidate);
      document.removeEventListener('visibilitychange', eligibilityChanged);
    };
  }, [blocked]);

  useEffect(() => {
    if (!open || blocked) return;
    const previous = document.activeElement;
    focusRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, [open, blocked]);

  // The eligibility gate revokes interaction immediately; keep only the inert
  // last frame long enough to fade rather than flashing away.
  const closeReview = () => {
    if (busyRef.current) return;
    dismissed.current = review;
    setReview(null);
    setError(undefined);
  };
  const acceptReview = async () => {
    if (!review || busyRef.current) return;
    const scope = encryptedSyncScope(review);
    const request = checkRef.current;
    busyRef.current = true;
    setBusy(true);
    setError(undefined);
    try {
      const prepared = await prepareEncryptedSync(scope);
      if (request !== checkRef.current || encryptedSyncScope(reviewRef.current ?? review) !== scope)
        return;
      setReview(null);
      if (prepared.nextStep !== 'ready') onSetup();
    } catch (failure) {
      if (request === checkRef.current)
        setError(t(`cloudSyncE2ee.errors.${encryptedSyncErrorKey(failure)}`));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };
  if (review && !blocked)
    return (
      <CloudSyncTermsDialog
        key={encryptedSyncScope(review)}
        updated
        busy={busy}
        error={error}
        onClose={closeReview}
        onConfirm={() => void acceptReview()}
      />
    );
  return (
    <PresenceModal
      open={open && !blocked}
      onClose={() => setOpen(false)}
      width="min(480px, 100%)"
      zIndex={75}
      render={() => (
        <section
          role="dialog"
          aria-modal="true"
          aria-labelledby="sync-setup-title"
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              event.preventDefault();
              event.stopPropagation();
              setOpen(false);
            }
            if (event.key === 'Tab') {
              const buttons = [
                ...event.currentTarget.querySelectorAll<HTMLButtonElement>('button'),
              ];
              const last = buttons[buttons.length - 1];
              const next = event.shiftKey ? last : buttons[0];
              if (document.activeElement === (event.shiftKey ? buttons[0] : last)) {
                event.preventDefault();
                next?.focus();
              }
            }
          }}
        >
          <CloudIcon
            size={28}
            aria-hidden="true"
            style={{ color: 'var(--ol-blue)', marginBottom: 16 }}
          />
          <h2 id="sync-setup-title" style={{ fontSize: 20, margin: '0 0 8px' }}>
            {t('cloudSyncE2ee.setupPromptTitle')}
          </h2>
          <p style={{ color: 'var(--ol-ink-3)', lineHeight: 1.65 }}>
            {t('cloudSyncE2ee.setupPromptBody')}
          </p>
          <div
            style={{
              display: 'flex',
              flexWrap: 'wrap',
              justifyContent: 'flex-end',
              gap: 12,
              marginTop: 20,
            }}
          >
            <button
              type="button"
              className="ol-tool-button"
              ref={focusRef}
              onClick={() => setOpen(false)}
            >
              {t('cloudSyncE2ee.setupPromptLater')}
            </button>
            <button
              type="button"
              className="ol-tool-button is-primary"
              onClick={() => {
                setOpen(false);
                onSetup();
              }}
            >
              {t('cloudSyncE2ee.setupPromptOpen')}
            </button>
          </div>
        </section>
      )}
    />
  );
}

/** Available before API-key setup. Opening this never enables sync or uploads. */
export function CloudSyncWelcome({ inline = false }: { inline?: boolean }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="ol-tool-button"
        disabled={!isTauriHost()}
        onClick={() => setOpen(true)}
        style={inline ? undefined : { position: 'fixed', top: 18, right: 18, zIndex: 10 }}
      >
        <CloudIcon size={16} />
        {t('cloudSync.restore')}
      </button>
      <PresenceModal
        open={Boolean(open)}
        onClose={() => setOpen(false)}
        width="min(680px, 100%)"
        zIndex={50}
        render={() =>
          open && (
            <>
              <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 12 }}>
                <button
                  type="button"
                  className="ol-tool-button"
                  aria-label={t('common.close')}
                  onClick={() => setOpen(false)}
                >
                  <XIcon size={18} />
                </button>
              </div>
              <CloudSyncSection />
            </>
          )
        }
      />
    </>
  );
}
