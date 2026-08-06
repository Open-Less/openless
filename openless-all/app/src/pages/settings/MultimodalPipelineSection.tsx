// 高级 → 实验性：多模态识别管线（issue #902）总开关。
// 开启后在 服务 → AI 提供商 顶部出现「传统模式 / 多模态模式」切换：
// 多模态模式用一个模型一步完成「提示词 + 音频 → 最终文本」，
// 凭据走独立 omni 命名空间，与传统 ASR/LLM 两套配置完全隔离、并存但停用。

import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle } from './shared';

export function MultimodalPipelineSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const onToggle = (multimodalPipelineEnabled: boolean) => {
    void updatePrefs({ ...prefs, multimodalPipelineEnabled }).catch(error => {
      console.error('[settings] failed to update multimodal pipeline flag', error);
    });
  };

  return (
    <Card>
      <SectionTitle hint={t('settings.advanced.multimodalPipelineTitleHint')}>
        {t('settings.advanced.multimodalPipelineTitle')}
      </SectionTitle>
      <SettingRow
        label={t('settings.advanced.multimodalPipelineLabel')}
        desc={t('settings.advanced.multimodalPipelineHint')}
      >
        <Toggle on={prefs.multimodalPipelineEnabled} onToggle={onToggle} />
      </SettingRow>
    </Card>
  );
}
