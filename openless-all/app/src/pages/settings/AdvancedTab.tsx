// AdvancedTab — 设置弹窗「高级」：本地模型 · 调试工具 · Beta / 自动更新。

import { useEffect, useState } from 'react';
import { AutoUpdateSection } from './AutoUpdateSection';
import { BetaChannelSection } from './BetaChannelSection';
import { ClaudeConsoleSection } from './ClaudeConsoleSection';
import { CodingAgentSection } from './CodingAgentSection';
import { DebugToolsSection } from './DebugToolsSection';
import { LocalModelSection } from './LocalModelSection';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import type { PlatformCapabilities } from '../../lib/types';

export function AdvancedTab() {
  const os = detectOS();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const showDesktopAdvanced = platformCaps?.platform === 'desktop';

  return (
    <>
      {showDesktopAdvanced && <LocalModelSection />}
      {showDesktopAdvanced && <DebugToolsSection />}
      {showDesktopAdvanced && os !== 'win' && <CodingAgentSection />}
      {showDesktopAdvanced && os !== 'win' && <ClaudeConsoleSection />}
      {platformCaps?.supportsAutoUpdate === true && <BetaChannelSection />}
      {platformCaps?.supportsAutoUpdate === true && <AutoUpdateSection />}
    </>
  );
}
