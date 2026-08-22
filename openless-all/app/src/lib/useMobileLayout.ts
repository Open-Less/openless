import { useEffect, useState } from 'react';
import { detectOS } from '../components/WindowChrome';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

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

/** 排版堆叠：移动端壳层 或 用户开启「易读布局」。用于 grid/flex 换行，不含底栏等壳层逻辑。 */
export function useLayoutStack(breakpoint = 720): boolean {
  const mobile = useMobileLayout(breakpoint);
  const { prefs } = useHotkeySettings();
  return mobile || prefs?.stackedRowLayout === true;
}
