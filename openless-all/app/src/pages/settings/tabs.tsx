// 设置弹窗里每个侧栏 tab 对应的内容页。每个 tab 就是若干 section 卡片的纵向堆叠；
// 真正的逻辑都在各 *Section 文件里，这里只负责"哪些 section 归到哪个 tab"。

import { useTranslation } from 'react-i18next';
import { useEffect, useState } from 'react';
import { RecordingInputSection } from './RecordingInputSection';
import { ShortcutsSection } from './ShortcutsSection';
import { LanguageSection } from './LanguageSection';
import { ThemeSection } from './ThemeSection';
import { ProvidersSection } from './ProvidersSection';
import { MarketplaceSection } from './MarketplaceSection';
import { PermissionsSection } from './PermissionsSection';
import { DataStorageSection } from './DataStorageSection';
import { LocalModelSection } from './LocalModelSection';
import { DebugToolsSection } from './DebugToolsSection';
import { CodingAgentSection } from './CodingAgentSection';
import { ClaudeConsoleSection } from './ClaudeConsoleSection';
import { BetaChannelSection } from './BetaChannelSection';
import { AutoUpdateSection } from './AutoUpdateSection';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import type { PlatformCapabilities } from '../../lib/types';

// 通用：录音与输入 · 快捷键 · 语言。
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

// 服务：AI 提供商 · 扩展市场。
export function ServicesTab() {
  return (
    <>
      <ProvidersSection />
      <MarketplaceSection />
    </>
  );
}

// 隐私：本地优先说明 + 权限管理 · 数据存储。
export function PrivacyTab() {
  const { t } = useTranslation();
  return (
    <>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 10,
          padding: '10px 12px',
          borderRadius: 10,
          background: 'var(--ol-blue-soft)',
          marginBottom: 2,
        }}
      >
        <span style={{
          fontSize: 11, padding: '3px 8px', borderRadius: 999,
          background: 'var(--ol-surface)',
          color: 'var(--ol-blue)', fontWeight: 600, flexShrink: 0,
        }}>
          {t('modal.about.localFirst')}
        </span>
        <span style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
          {t('modal.about.privacyDesc')}
        </span>
      </div>
      <PermissionsSection />
      <DataStorageSection />
    </>
  );
}

// 高级：本地模型 · 调试工具 · 加入 Beta 渠道（固定在最下面）。
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
