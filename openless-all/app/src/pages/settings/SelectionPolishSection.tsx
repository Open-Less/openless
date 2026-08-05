// 通用 → 选区润色：这是与录音输入并列的独立入口，快捷键、交付方式均不再藏在快捷键列表中。

import type { PlatformCapabilities, SelectionPolishOutputMode } from '../../lib/types';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { defaultSelectionPolishShortcut } from '../../lib/hotkey';
import { setSelectionPolishHotkey } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, chipSelectedStyle, segmentedTrackStyle } from './shared';

const outputOptions: Array<{ value: SelectionPolishOutputMode }> = [
  { value: 'directReplace' },
  { value: 'previewConfirm' },
];

export function SelectionPolishSection() {
  const { t } = useTranslation();
  const { prefs, capability, refresh, updatePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => { void getPlatformCapabilities().then(setPlatformCaps); }, []);

  // 选区润色的安全替换：Windows 用前台窗口/焦点控件校验，macOS 用前台应用 +
  // 选区文本指纹校验；两者都具备后才提供设置入口（Linux 热键接入后同样可用）。
  if (!prefs || !capability || !platformCaps?.supportsDesktopHotkey) return null;

  return (
    <Card>
      <SectionTitle hint={t('settings.selectionPolish.hint')}>
        {t('settings.selectionPolish.title', '选区润色')}
      </SectionTitle>
      <SettingRow
        label={t('settings.selectionPolish.hotkey', '触发快捷键')}
        desc={t('settings.selectionPolish.hotkeyDesc')}
      >
        <ShortcutRecorder
          value={prefs.selectionPolishHotkey}
          sideSpecificModifiers
          onSave={async binding => {
            await setSelectionPolishHotkey(binding);
            await refresh();
          }}
          onDisable={async () => {
            await setSelectionPolishHotkey(null);
            await refresh();
          }}
          // 「重置」= 恢复默认快捷键，等价于旧版独立「启用」按钮。
          onReset={async () => {
            await setSelectionPolishHotkey(defaultSelectionPolishShortcut());
            await refresh();
          }}
        />
      </SettingRow>
      <SettingRow label={t('settings.selectionPolish.delivery', '结果处理方式')}>
        <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
          {outputOptions.map(option => {
            const selected = prefs.selectionPolishOutputMode === option.value;
            return (
              <button
                key={option.value}
                title={t(`settings.selectionPolish.${option.value}Hint`)}
                onClick={() => void updatePrefs(current => ({ ...current, selectionPolishOutputMode: option.value }))}
                style={{
                  ...chipSelectedStyle(selected), border: 0, borderRadius: 6, padding: '6px 10px',
                  fontFamily: 'inherit', fontSize: 12, cursor: 'default', fontWeight: selected ? 600 : 500,
                }}
              >
                {t(`settings.selectionPolish.${option.value}`)}
              </button>
            );
          })}
        </div>
      </SettingRow>
    </Card>
  );
}
