import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CloudIcon, XIcon } from 'lucide-react';
import { isTauri } from '../lib/ipc/shared';
import { cloudSyncE2eeClaimSetupPrompt } from '../lib/ipc/cloud-sync-e2ee';
import { CloudSyncSection } from '../pages/settings/CloudSyncSection';
import { Modal } from './ui/Modal';

const PROMPT_EVENTS = new Set([
  'backend_started',
  'preferences_changed',
  'credentials_changed',
  'dictation_completed',
  'dictation_state_changed',
  'qa_state',
  'selection_state_changed',
  'selection_voice_state_changed',
  'less_computer_event',
  'local_asr_engine_changed',
  'permission_changed',
]);

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
  const checkRef = useRef(0);
  const focusRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!isTauri || blocked) return;
    let cancelled = false;
    let inFlight = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unlisten: (() => void) | undefined;
    const check = async () => {
      if (cancelled || inFlight || document.visibilityState !== 'visible' || !document.hasFocus())
        return;
      inFlight = true;
      const request = ++checkRef.current;
      try {
        const claimed = await cloudSyncE2eeClaimSetupPrompt();
        if (
          !cancelled &&
          checkRef.current === request &&
          claimed &&
          document.visibilityState === 'visible' &&
          document.hasFocus()
        )
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
      checkRef.current += 1;
      setOpen(false);
    };
    const eligibilityChanged = () => {
      invalidate();
      schedule();
    };
    void import('@tauri-apps/api/event')
      .then(async ({ listen }) => {
        const handle = await listen<{ kind: { type: string } }>('backend:event', ({ payload }) => {
          if (PROMPT_EVENTS.has(payload.kind.type)) eligibilityChanged();
        });
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

  if (!open || blocked) return null;
  return (
    <Modal onClose={() => setOpen(false)} width="min(480px, 100%)" zIndex={75}>
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
            const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('button')];
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
    </Modal>
  );
}

/** Available before API-key setup. Opening this never enables sync or uploads. */
export function CloudSyncWelcome() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="ol-tool-button"
        disabled={!isTauri}
        onClick={() => setOpen(true)}
        style={{ position: 'fixed', top: 18, right: 18, zIndex: 10 }}
      >
        <CloudIcon size={16} />
        {t('cloudSync.restore')}
      </button>
      {open && (
        <Modal onClose={() => setOpen(false)} width="min(680px, 100%)" zIndex={50}>
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
        </Modal>
      )}
    </>
  );
}
