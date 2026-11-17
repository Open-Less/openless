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
import { installImeEnterGuard } from './lib/imeEnterGuard';
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
// Browser preview on :1420 keeps the main window's 1300×835 layout at zoom 1.
// Shrinking it with CSS zoom makes Cursor's element overlay miss the painted
// controls. The pane scrolls when it is smaller than the window. Native
// webviews and Playwright do not set this frame.
const browserFrame =
  isMainWindow &&
  !('__TAURI_INTERNALS__' in window) &&
  navigator.webdriver !== true &&
  location.port === '1420';
const MAIN_WINDOW_WIDTH = 1300;
const MAIN_WINDOW_HEIGHT = 835;

if (browserFrame) {
  document.documentElement.dataset.olBrowserFrame = 'main';
  document.documentElement.style.setProperty('--ol-browser-frame-width', `${MAIN_WINDOW_WIDTH}px`);
  document.documentElement.style.setProperty(
    '--ol-browser-frame-height',
    `${MAIN_WINDOW_HEIGHT}px`,
  );
  document.documentElement.style.setProperty('--ol-browser-frame-scale', '1');
}
initThemeMode();
initInteractionMode();
installImeEnterGuard();
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
