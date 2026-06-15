// GeneralTab — 设置弹窗「通用」：录音与输入 · 快捷键 · 主题 · 语言。

import { useEffect, useState } from 'react';
import { RecordingInputSection } from './RecordingInputSection';
import { ShortcutsSection } from './ShortcutsSection';
import { LanguageSection } from './LanguageSection';
import { ThemeSection } from './ThemeSection';
import { getPlatformCapabilities } from '../../lib/platform';
import type { PlatformCapabilities } from '../../lib/types';

export function GeneralTab() {
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const showDesktopShortcuts = platformCaps?.supportsDesktopHotkey === true;

  return (
    <>
      <RecordingInputSection />
      {showDesktopShortcuts && <ShortcutsSection />}
      <ThemeSection />
      <LanguageSection />
    </>
  );
}
