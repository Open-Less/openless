import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { i18nReady } from '../i18n';
import { applyFontScale, readFontScale } from '../lib/fontScale';
import { installImeEnterGuard } from '../lib/imeEnterGuard';
import { initInteractionMode } from '../lib/interactionMode';
import { afterPaint, markStartup } from '../lib/startupMetrics';
import { initThemeMode } from '../lib/themeMode';
import { initTypography } from '../lib/typography';
import { getPreviewProfile } from '../preview/profiles';
import { getRuntimeEnvironment } from '../runtime/environment';
import { App } from './App';
import { resolveWindowKind } from './windowRoutes';
import '../styles/tokens.css';
import '../styles/global.css';
import '../styles/overlays.css';
import '../styles/refinements.css';

export function bootstrap() {
  const params = new URLSearchParams(location.search);
  const kind = resolveWindowKind(params);
  const environment = getRuntimeEnvironment();
  const rootElement = document.documentElement;
  rootElement.dataset.olPlatform = environment.os;
  rootElement.toggleAttribute('data-ol-aux', kind !== 'main');

  const framed =
    environment.host === 'preview' &&
    kind === 'main' &&
    (params.get('frame') === '1' ||
      (params.get('frame') !== '0' && location.port === '1420' && navigator.webdriver !== true));
  if (framed) rootElement.dataset.olBrowserFrame = 'main';
  const previewFrame = framed ? getPreviewProfile(environment.os).viewport : undefined;

  initThemeMode();
  initInteractionMode();
  installImeEnterGuard();
  initTypography();
  if (kind === 'main') applyFontScale(readFontScale());
  markStartup('frontend');

  const root = createRoot(document.getElementById('root')!);
  const render = (languageError?: string) =>
    root.render(
      <StrictMode>
        <App
          kind={kind}
          os={environment.os}
          previewFrame={previewFrame}
          previewOnboarding={environment.host === 'preview' && params.get('onboarding') === '1'}
          languageError={languageError}
        />
      </StrictMode>,
    );

  // The selected language chunk must be ready before any React screen is shown.
  void i18nReady
    .then(() => {
      render();
      afterPaint('react');
    })
    .catch((error: unknown) => render(String(error)));
}
