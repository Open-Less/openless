import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { TooltipProvider } from './components/Tooltip';
import { afterPaint, markStartup } from './lib/startupMetrics';
import { initInteractionMode } from './lib/interactionMode';
import { applyFontScale, readFontScale } from './lib/fontScale';
import { initTypography } from './lib/typography';
import { CoreStartupScreen } from './components/CoreStartupScreen';
import { detectOS } from './components/WindowChrome';
import { i18nReady } from './i18n';
import { initThemeMode } from './lib/themeMode';
import { installEncryptedSyncUiBridge } from './lib/encryptedSyncUiBridge';
import './styles/tokens.css';
import './styles/global.css';
import './styles/overlays.css';
import './styles/refinements.css';

import type { OS } from './components/WindowChrome';

const params = new URLSearchParams(window.location.search);
const windowKind = params.get('window');
const isCapsule = windowKind === 'capsule';
const isCapsuleRail = windowKind === 'capsule-rail';
const isQa = windowKind === 'qa';
const isSelectionVoiceIntent = windowKind === 'selection-voice-intent';
const isVoiceEdit = windowKind === 'voice-edit';
const isLessComputer = windowKind === 'less-computer';
const isLessComputerGlow = windowKind === 'less-computer-glow';
// Auxiliary windows share the bundle but never run main-window startup work.
const isMainWindow = !windowKind;
const osQuery = params.get('os') as OS | null;
const os = osQuery ?? detectOS();
document.documentElement.dataset.olPlatform = os;
initThemeMode();
initInteractionMode();
initTypography();
if (isMainWindow) applyFontScale(readFontScale());
markStartup('frontend');

const root = ReactDOM.createRoot(document.getElementById('root')!);

const renderApp = () => {
  root.render(
    <React.StrictMode>
      <TooltipProvider>
        <App
          isCapsule={isCapsule}
          isCapsuleRail={isCapsuleRail}
          isQa={isQa}
          isSelectionVoiceIntent={isSelectionVoiceIntent}
          isVoiceEdit={isVoiceEdit}
          isLessComputer={isLessComputer}
          isLessComputerGlow={isLessComputerGlow}
          forcedOs={os}
        />
      </TooltipProvider>
    </React.StrictMode>,
  );
};

// Mount only after the selected local language chunk is ready; avoid mixed-language startup.
void i18nReady
  .then(() => {
    // Start listening before rendering, but don't hold the first frame behind Keychain I/O.
    if (isMainWindow)
      void installEncryptedSyncUiBridge()
        .then(() => markStartup('sync'))
        .catch(() => {});
    renderApp();
    afterPaint('react');
  })
  .catch((error: unknown) => {
    root.render(
      <CoreStartupScreen error={String(error)} compact={isCapsule || isLessComputerGlow} />,
    );
  });
