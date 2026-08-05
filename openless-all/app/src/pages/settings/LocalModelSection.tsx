// 服务 → 本地模型：本地 ASR 模型的管理入口。含 Qwen3（macOS）/ Foundry Local +
// sherpa-onnx（Windows）三条本地引擎。
//
// 2026-08 重构：本地 ASR 不再有「启用开关」——本地 ASR 始终可用，是否激活由
// 「服务 → AI 提供商」的 ASR 语音转写供应商决定：选到本地模型供应商即使用本地
// 引擎（与 Apple 语音同理）。这里只保留模型下载 / 管理看板（<LocalAsr embedded />）。

import { useEffect, useState } from 'react';
import type { PlatformCapabilities } from '../../lib/types';
import { useTranslation } from 'react-i18next';
import { LocalAsr } from '../LocalAsr';
import { detectOS } from '../../components/WindowChrome';
import { getPlatformCapabilities } from '../../lib/platform';
import { Card } from '../_atoms';

export function LocalModelSection() {
  const { t } = useTranslation();
  const os = detectOS();
  const isWin = os === 'win';
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  const platformSupported = platformCaps?.supportsLocalAsr === true;

  return (
    <Card>
      {/* 标题 + 右上角 inline 警告小字（实验性标记保留）。
          Windows：标题区整体灰显 —— 本地 ASR 在 Win 上走 Foundry / sherpa 独立路径。 */}
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 14 }}>
        <div style={{ minWidth: 0, opacity: isWin ? 0.45 : 1 }}>
          <div style={{ fontSize: 14, fontWeight: 600, letterSpacing: '-0.01em' }}>{t('settings.advanced.localAsrTitle')}</div>
        </div>
        <div style={{
          fontSize: 11,
          color: '#A04500',
          fontWeight: 500,
          lineHeight: 1.4,
          textAlign: 'right',
          flexShrink: 0,
          maxWidth: '52%',
          paddingTop: 2,
          opacity: isWin ? 0.45 : 1,
        }}>
          ⚠️ {t('settings.advanced.localAsrWarningShort')}
        </div>
      </div>

      {!platformSupported ? (
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-3)', lineHeight: 1.6, padding: '8px 0' }}>
          {t('settings.advanced.platformNotSupported')}
        </div>
      ) : (
        /* 模型下载 / 管理看板（模型选择 · 下载 · 删除 · 测试 · 镜像源收纳）——
           较大的内嵌框。启用入口在「AI 提供商 → ASR 语音转写」选择本地供应商。 */
        <div style={{ marginTop: 16, borderTop: '0.5px solid var(--ol-line)', paddingTop: 16 }}>
          <LocalAsr embedded />
        </div>
      )}
    </Card>
  );
}
