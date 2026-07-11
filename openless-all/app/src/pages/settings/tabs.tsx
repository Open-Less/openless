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
import { AboutSection } from './AboutSection';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import type { PlatformCapabilities } from '../../lib/types';

// 各 tab 共用的平台能力查询（决定桌面/移动、是否支持热键与自动更新等 gating）。
function usePlatformCaps(): PlatformCapabilities | null {
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  return platformCaps;
}

// 通用：录音与输入 · 快捷键 · 主题 · 语言。
export function GeneralTab() {
  const platformCaps = usePlatformCaps();
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

// 服务：AI 提供商 · 本地模型 · 扩展市场。
// 本地模型是「语音识别由谁提供」的一种答案，和云端提供商属同一决策，
// 不再藏进「高级」。
export function ServicesTab() {
  const platformCaps = usePlatformCaps();
  const showLocalModel = platformCaps?.platform === 'desktop';

  return (
    <>
      <ProvidersSection />
      {showLocalModel && <LocalModelSection />}
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

// 高级：只留真正的实验性/开发者功能 —— Less Computer · Claude 控制台 · 调试工具。
// （本地模型移入「服务」、更新相关移入「关于」，这个 tab 不再是杂物抽屉。）
export function AdvancedTab() {
  const os = detectOS();
  const platformCaps = usePlatformCaps();
  const showDesktopAdvanced = platformCaps?.platform === 'desktop';

  return (
    <>
      {showDesktopAdvanced && os !== 'win' && <CodingAgentSection />}
      {showDesktopAdvanced && os !== 'win' && <ClaudeConsoleSection />}
      {showDesktopAdvanced && <DebugToolsSection />}
    </>
  );
}

// 关于：版本信息 · 更新渠道 · 自动更新 —— 「我用的是什么版本、怎么更新」归一处。
export function AboutTab() {
  const platformCaps = usePlatformCaps();
  const showUpdateControls = platformCaps?.supportsAutoUpdate === true;

  return (
    <>
      <AboutSection />
      {showUpdateControls && <BetaChannelSection />}
      {showUpdateControls && <AutoUpdateSection />}
    </>
  );
}
