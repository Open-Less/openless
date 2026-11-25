import { useLayoutEffect, useState } from 'react';
import { getRuntimeEnvironment } from '../runtime/environment';
import { useAppViewport } from '../ui/layout/AppViewport';

export function shouldUseMobileLayout(breakpoint = 720, viewport?: HTMLElement | null): boolean {
  if (typeof window === 'undefined') return false;
  if (getRuntimeEnvironment().isMobile) return true;
  if (!viewport) return window.innerWidth < breakpoint;
  const scale = Number(getComputedStyle(viewport).getPropertyValue('--ol-interface-scale')) || 1;
  return viewport.clientWidth / scale < breakpoint;
}

export function useMobileLayout(breakpoint = 720): boolean {
  const { boundary } = useAppViewport();
  const [mobile, setMobile] = useState(() => shouldUseMobileLayout(breakpoint, boundary));

  useLayoutEffect(() => {
    const sync = () => setMobile(shouldUseMobileLayout(breakpoint, boundary));
    sync();
    const observer =
      boundary && typeof ResizeObserver !== 'undefined' ? new ResizeObserver(sync) : null;
    if (observer && boundary) observer.observe(boundary);
    window.addEventListener('resize', sync);
    window.addEventListener('orientationchange', sync);
    window.addEventListener('openless:ui-preferences-changed', sync);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', sync);
      window.removeEventListener('orientationchange', sync);
      window.removeEventListener('openless:ui-preferences-changed', sync);
    };
  }, [breakpoint, boundary]);

  return mobile;
}

/** Legacy row consumers remain responsive, but retired layout preferences are ignored. */
export function useReadableLayout(): boolean {
  return false;
}

/** Narrow screens stack controls automatically. */
export function useLayoutStack(breakpoint = 720): boolean {
  return useMobileLayout(breakpoint);
}

export function useConservativeLayout(): boolean {
  return false;
}
