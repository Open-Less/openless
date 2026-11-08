import { lazy, Suspense, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import { PresenceModal } from './ui/Modal';
import { PageSkeleton } from './ui/PageSkeleton';
import { ProviderLeaveContext, useProviderForm } from '../pages/settings/ProviderForm';
import { ChannelEditorHostContext } from '../pages/settings/ChannelEditorHostContext';
import { useSmoothWheelScroll } from '../lib/useSmoothWheelScroll';

const ProvidersSection = lazy(() =>
  import('../pages/settings/ChannelList').then((module) => ({ default: module.ProvidersSection })),
);
const OmniChannelSection = lazy(() =>
  import('../pages/settings/ProvidersSection').then((module) => ({
    default: module.OmniChannelSection,
  })),
);

/** A bounded setup workspace: list and editor share this dialog's frame, while
 * the underlying guide retains its step, focus position and unsaved choices. */
export function ModelSetupDialog({
  open,
  kind,
  onClose,
}: {
  open: boolean;
  kind: 'asr' | 'llm' | 'omni';
  onClose: () => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const form = useProviderForm();
  const [container, setContainer] = useState<HTMLDivElement | null>(null);
  const [background, setBackground] = useState<HTMLDivElement | null>(null);
  const editorClose = useRef<(() => void) | null>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const [error, setError] = useState('');
  useSmoothWheelScroll(scroll, `${open}`);
  const close = async () => {
    if (editorClose.current) {
      editorClose.current();
      return;
    }
    setError('');
    try {
      if (!(await form.finish(onClose))) setError(t('desktopGuide.saveError'));
    } catch {
      setError(t('desktopGuide.saveError'));
    }
  };
  return (
    <ProviderLeaveContext.Provider value={form.register}>
      <PresenceModal
        open={open}
        onClose={() => void close()}
        width="min(820px, 100%)"
        labelledBy="guide-model-dialog-title"
        zIndex={200}
        style={{ padding: 0, overflow: 'hidden', height: 'min(680px, 100%)', maxHeight: '100%' }}
        render={() => (
          <div className="ol-model-setup-dialog">
            <ChannelEditorHostContext.Provider
              value={{
                presentation: 'inline',
                container,
                background,
                registerClose: (close) => {
                  editorClose.current = close;
                },
              }}
            >
              <div ref={setBackground} className="ol-model-setup-background">
                <header className="ol-model-setup-header">
                  <div>
                    <h2 id="guide-model-dialog-title">
                      {t(
                        `desktopGuide.${kind === 'asr' ? 'recognitionTitle' : kind === 'llm' ? 'polishTitle' : 'omniTitle'}`,
                      )}
                    </h2>
                    <p>{t('settings.channels.autoSaveHint')}</p>
                  </div>
                  <button
                    type="button"
                    className="ol-guide-secondary"
                    aria-label={t('common.close')}
                    disabled={form.leaving}
                    onClick={() => void close()}
                  >
                    <X size={18} />
                  </button>
                </header>
                <div className="ol-model-setup-body ol-thinscroll" ref={scroll}>
                  <Suspense fallback={<PageSkeleton rows={3} />}>
                    {kind === 'omni' ? <OmniChannelSection /> : <ProvidersSection kind={kind} />}
                  </Suspense>
                </div>
                <footer className="ol-model-setup-footer">
                  <span role={error ? 'alert' : undefined}>{error || t('modal.autoSaveHint')}</span>
                  <button
                    type="button"
                    className="ol-guide-primary"
                    onClick={() => void close()}
                    disabled={form.leaving}
                  >
                    {t('settings.channels.done')}
                  </button>
                </footer>
              </div>
              <div ref={setContainer} className="ol-model-setup-editor" />
            </ChannelEditorHostContext.Provider>
          </div>
        )}
      />
    </ProviderLeaveContext.Provider>
  );
}
