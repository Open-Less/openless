import { invokeOrMock } from './shared';
import { APP_VERSION } from '../appVersion';

/** Generation marker matching the first segment of `CARGO_PKG_VERSION` in Rust's
 *  `take_splash_playback`. */
export const SPLASH_MAJOR = APP_VERSION.split('.')[0] ?? '0';

const MOCK_SPLASH_MARKER_KEY = 'openless.splashSeenVersion';

/** Process-level decision cache: StrictMode double mounts / component remounts reuse the same
 *  consumed result — "play or not" asks Rust only once per webview process lifetime. */
let splashDecision: Promise<boolean> | null = null;

/**
 * Consume the "first launch of this major version" splash-video marker: true = first launch of
 * this generation, so the frontend should play the bundled splash animation fullscreen once;
 * false = the config already carries this generation's marker and it never plays again. In a real
 * environment Rust reads/writes preferences.json; browser dev mode uses localStorage to mimic the
 * same "play once, never again" semantics.
 */
export function takeSplashPlayback(): Promise<boolean> {
  splashDecision ??= invokeOrMock<boolean>('take_splash_playback', undefined, () => {
    if (window.localStorage.getItem(MOCK_SPLASH_MARKER_KEY) === SPLASH_MAJOR) {
      return false;
    }
    window.localStorage.setItem(MOCK_SPLASH_MARKER_KEY, SPLASH_MAJOR);
    return true;
  });
  return splashDecision;
}
