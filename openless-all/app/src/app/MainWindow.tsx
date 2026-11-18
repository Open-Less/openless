import { lazy, Suspense } from 'react';
import { useTranslation } from 'react-i18next';
import { HotkeySettingsProvider } from '../data/preferences';
import { useReducedMotion } from '../lib/motion';
import type { OS } from '../runtime/environment';
import { CoreStartupScreen } from './CoreStartupScreen';
import { useAndroidPanels } from './useAndroidPanels';
import { useMainStartup } from './startup';
import { useMainWindowHotkeys } from './windowHotkeys';
import { Shell, QaPanel, VoiceEditPanel } from './destinations';
import './main-window.css';

const AutoUpdateGate = lazy(() =>
  import('../features/updates/AutoUpdateGate').then((module) => ({
    default: module.AutoUpdateGate,
  })),
);
const Onboarding = lazy(() =>
  import('../features/onboarding/Onboarding').then((module) => ({ default: module.Onboarding })),
);

export function MainWindow({ os, previewOnboarding }: { os: OS; previewOnboarding: boolean }) {
  const { t } = useTranslation();
  const { gate, caps, error, onboardingHandoff, completeOnboarding } = useMainStartup(
    os,
    previewOnboarding,
    useReducedMotion(),
  );
  const { panel, close, didClose } = useAndroidPanels(caps?.platform === 'android');
  useMainWindowHotkeys(os);
  if (gate === 'checking' || gate === 'failed') return <CoreStartupScreen error={error} />;

  return (
    <HotkeySettingsProvider>
      <Suspense fallback={<CoreStartupScreen />}>
        {panel.kind === 'qa' ? (
          <div key={panel.epoch} className="ol-embedded-panel">
            <QaPanel
              onRequestDismiss={() => close(panel)}
              embedded
              onRequestClose={() => didClose(panel)}
            />
            {panel.error && (
              <div role="alert" className="ol-mobile-panel-error">
                {t('qa.compact.closeError')} {panel.error}
              </div>
            )}
          </div>
        ) : panel.kind === 'voice-edit' ? (
          <div key={panel.epoch} className="ol-embedded-panel">
            <VoiceEditPanel
              embedded
              closeError={panel.error}
              onRequestClose={() => didClose(panel)}
              onRequestDismiss={() => close(panel)}
            />
          </div>
        ) : (
          <div style={{ width: '100%', height: '100%', minWidth: 0, position: 'relative' }}>
            {gate === 'ready' && (
              <div className="ol-main-arrival">
                <Shell os={os} />
              </div>
            )}
            {(gate === 'onboarding' || onboardingHandoff) && (
              <div
                className="ol-onboarding-layer"
                data-leaving={onboardingHandoff || undefined}
                aria-hidden={onboardingHandoff || undefined}
                ref={(element) => {
                  if (element) element.inert = onboardingHandoff;
                }}
              >
                <Onboarding onComplete={completeOnboarding} />
              </div>
            )}
          </div>
        )}
      </Suspense>
      {gate === 'ready' && caps?.supportsAutoUpdate === true && (
        <Suspense fallback={null}>
          <AutoUpdateGate />
        </Suspense>
      )}
    </HotkeySettingsProvider>
  );
}
