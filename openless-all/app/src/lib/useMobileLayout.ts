import { useEffect, useState } from 'react';
import { detectOS } from '../components/WindowChrome';

export function shouldUseMobileLayout(breakpoint = 720): boolean {
  if (typeof window === 'undefined') return false;
  const osQuery = new URLSearchParams(window.location.search).get('os');
  return osQuery === 'android' || detectOS() === 'android' || window.innerWidth < breakpoint;
}

export function useMobileLayout(breakpoint = 720): boolean {
  const [mobile, setMobile] = useState(() => shouldUseMobileLayout(breakpoint));

  useEffect(() => {
    const sync = () => setMobile(shouldUseMobileLayout(breakpoint));
    sync();
    window.addEventListener('resize', sync);
    window.addEventListener('orientationchange', sync);
    return () => {
      window.removeEventListener('resize', sync);
      window.removeEventListener('orientationchange', sync);
    };
  }, [breakpoint]);

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
