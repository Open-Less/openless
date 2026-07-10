import { useEffect, useState } from 'react';
import { detectOS } from '../components/WindowChrome';

function shouldUseMobileLayout(breakpoint: number): boolean {
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
