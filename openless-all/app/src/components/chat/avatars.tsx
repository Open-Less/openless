// avatars.tsx — thinking animation and avatars.
//
// · ThinkingOrb: the exact same implementation as the capsule's thinking state
//   (SiriGL mode="orb", 6 white metaball dots weaving and rotating, thinking
//   speed=1.5 matching Capsule.tsx). White dots need a dark backdrop — carried by the
//   small dark stage (olchat-orb-stage) to contrast with the panel's off-white
//   background. Falls back to a static white-dot ring when WebGL is unavailable.
// · StaticOrbDots: static white-dot ring for settled message avatars (no GL context —
//   a live orb per message in long chats would hit the browser's WebGL context limit).
// · UserAvatar: when logged into GitHub (prefs.marketplaceDevLogin), shows the
//   github.com/{login}.png avatar; falls back to the GitHub icon when logged out or
//   on load failure. Used only by selection ask (QA).

import { useEffect, useRef, useState } from 'react';
import { ThinkingDots } from '../ThinkingDots';
import { SiriGL, isWebGLAvailable } from '../SiriGL';
import { subscribeOrbFrames } from './orbFeed';
import { getSettings } from '../../lib/ipc/settings';
import { isTauri } from '../../lib/ipc/shared';
import { cn } from './lib/utils';
import './chat.css';

/** Capsule-identical thinking animation (SiriGL orb on a dark stage). */
export function ThinkingOrb({ size = 56, className }: { size?: number; className?: string }) {
  return (
    <span
      className={cn('olchat-orb-stage', className)}
      style={{ width: size, height: size }}
      aria-hidden
    >
      {isWebGLAvailable() ? (
        <SiriGL mode="orb" speed={1.5} />
      ) : (
        <ThinkingDots
          size={size * 0.5}
          style={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            transform: 'translate(-50%, -50%)',
          }}
        />
      )}
    </span>
  );
}

/**
 * Capsule thinking animation as an avatar: the rotating orb, identical to the capsule
 * (every assistant message avatar spins). No per-avatar GL context — each frame is
 * mirrored from the shared render source (orbFeed, the panel's single GL context) to
 * a small 2D canvas via drawImage. Falls back to a ThinkingDots ring without WebGL.
 */
export function OrbAvatar({ size = 32 }: { size?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [unavailable, setUnavailable] = useState(false);

  useEffect(() => {
    const target = canvasRef.current;
    if (!target) return undefined;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    target.width = Math.round(size * dpr);
    target.height = Math.round(size * dpr);
    const ctx = target.getContext('2d');
    if (!ctx) {
      setUnavailable(true);
      return undefined;
    }
    const unsubscribe = subscribeOrbFrames((source) => {
      // Center-crop and scale (the source is composed for the capsule; the halo takes
      // only ~1/3 of the frame — crop the outer padding so the ring fills the avatar,
      // animation unchanged).
      const crop = 0.62;
      const sw = source.width * crop;
      const sh = source.height * crop;
      const sx = (source.width - sw) / 2;
      const sy = (source.height - sh) / 2;
      ctx.clearRect(0, 0, target.width, target.height);
      ctx.drawImage(source, sx, sy, sw, sh, 0, 0, target.width, target.height);
    });
    if (!unsubscribe) {
      setUnavailable(true);
      return undefined;
    }
    return unsubscribe;
  }, [size]);

  if (unavailable) {
    return <ThinkingDots size={size * 0.56} />;
  }
  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      style={{ display: 'block', width: size, height: size, pointerEvents: 'none' }}
    />
  );
}

/**
 * Current GitHub login (Marketplace upload identity, written to prefs on sign-in in
 * Settings). Re-fetches when refreshKey changes — the panel window is a persistent
 * webview (reused across hide/show), so a new-session signal drives the refresh to
 * track login changes.
 */
export function useGithubLogin(refreshKey?: string | number): string {
  const [login, setLogin] = useState('');
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    getSettings()
      .then((prefs) => {
        if (!cancelled) setLogin((prefs.marketplaceDevLogin ?? '').trim());
      })
      .catch(() => {
        /* Not logged in / read failed: keep the icon fallback. */
      });
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);
  return login;
}

/** User avatar (QA only): GitHub avatar image, or the GitHub icon when logged out. */
export function UserAvatar({ login }: { login: string }) {
  const [broken, setBroken] = useState(false);
  useEffect(() => setBroken(false), [login]);
  const showImage = login.length > 0 && !broken;
  return showImage ? (
    <img
      className="size-8 rounded-full object-cover"
      src={`https://github.com/${encodeURIComponent(login)}.png?size=64`}
      alt=""
      loading="lazy"
      draggable={false}
      onError={() => setBroken(true)}
    />
  ) : (
    <span className="flex size-8 items-center justify-center text-foreground/70">
      <GithubMark />
    </span>
  );
}

function GithubMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}
