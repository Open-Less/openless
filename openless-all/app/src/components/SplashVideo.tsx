import { useEffect, useRef, useState } from 'react';
import { takeSplashPlayback } from '../lib/ipc';

/** Bundled 2.0 splash video (public/ static asset, Vite copies it into dist as-is). */
const SPLASH_SRC = '/openless-2.0-splash.mp4';
/** Fade duration for the whole layer after playback, matching global.css's ol-splash-out. */
const FADE_MS = 1200;
/** Watchdog: if both `ended` / `error` fail (corrupt encoding, background throttling),
 *  the splash must fade out after at most 25s — never trap the user behind the
 *  animation. */
const SPLASH_WATCHDOG_MS = 25_000;

type SplashPhase = 'pending' | 'playing' | 'fading' | 'done';

/**
 * 2.0 splash video: plays once, only on the first launch lacking this major version's
 * marker. The marker is read/written in preferences.json by Rust
 * `take_splash_playback` (browser dev mode emulates it with localStorage, same
 * semantics). The play decision happens at mount, so each webview process consumes
 * it exactly once.
 *
 * Presentation: full-screen cover (object-fit: cover crops to fill at any window
 * ratio, no letterboxing); after playback the whole layer fades from the last frame
 * to transparent (ol-splash-out).
 */
export function SplashVideo() {
  const [phase, setPhase] = useState<SplashPhase>('pending');
  const videoRef = useRef<HTMLVideoElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    takeSplashPlayback()
      .then((shouldPlay) => {
        if (!cancelled) setPhase(shouldPlay ? 'playing' : 'done');
      })
      .catch(() => {
        // Decision IPC failure = don't play. The splash is a garnish; it must never
        // block the app itself.
        if (!cancelled) setPhase('done');
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (phase !== 'playing') return;
    const watchdog = window.setTimeout(() => setPhase('fading'), SPLASH_WATCHDOG_MS);
    return () => window.clearTimeout(watchdog);
  }, [phase]);

  useEffect(() => {
    if (phase !== 'fading') return;
    const timer = window.setTimeout(() => setPhase('done'), FADE_MS);
    return () => window.clearTimeout(timer);
  }, [phase]);

  useEffect(() => {
    if (phase !== 'playing') return;
    const video = videoRef.current;
    if (!video) return;
    // autoPlay tries with sound first; when WKWebView rejects audio autoplay, fall
    // back to muted playback — the animation always completes, sound if allowed.
    video.play().catch(() => {
      video.muted = true;
      video.play().catch(() => setPhase('fading'));
    });
  }, [phase]);

  if (phase === 'pending' || phase === 'done') return null;
  return (
    <div
      className="ol-splash"
      data-fading={phase === 'fading' ? 'true' : undefined}
      role="presentation"
    >
      <video
        ref={videoRef}
        className="ol-splash-video"
        src={SPLASH_SRC}
        autoPlay
        playsInline
        onEnded={() => setPhase('fading')}
        onError={() => setPhase('fading')}
      />
    </div>
  );
}
