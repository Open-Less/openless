// 隐私 → 数据存储：本地保留的历史会话与对话上下文窗口。
// 自 Settings.tsx 的 RecordingSection「历史与上下文」折叠组拆出，逻辑零改动。

import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, inputStyle } from './shared';

// 范围限制：context window 0-60 分钟（再大对实际对话场景没意义且白烧 token）。
const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export function DataStorageSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();

  if (!prefs) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  // 空字符串 / 0 = 不限；输入框用文字占位，不把 0 展示成一个普通天数。
  const onHistoryRetentionChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, historyRetentionDays: Math.max(0, parsed) });
  };
  const onPolishContextWindowChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, polishContextWindowMinutes: clamp(parsed, 0, 60) });
  };
  // 空字符串 / 0 = 不限。
  const onHistoryMaxEntriesChange = (raw: string) => {
    const trimmed = raw.trim();
    if (trimmed === '' || trimmed === '0') {
      void savePrefs({ ...prefs, historyMaxEntries: null });
      return;
    }
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, historyMaxEntries: Math.max(1, parsed) });
  };

  return (
    <Card>
      <SectionTitle>{t('settings.dataStorage.title')}</SectionTitle>
      <SettingRow label={t('settings.recording.historyRetentionLabel')}>
        <input
          type="number"
          min={0}
          placeholder={t('common.unlimited')}
          value={prefs.historyRetentionDays === 0 ? '' : prefs.historyRetentionDays}
          onChange={e => onHistoryRetentionChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      <SettingRow label={t('settings.recording.historyMaxEntriesLabel')}>
        <input
          type="number"
          min={1}
          placeholder={t('common.unlimited')}
          value={prefs.historyMaxEntries ?? ''}
          onChange={e => onHistoryMaxEntriesChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      <SettingRow label={t('settings.recording.polishContextWindowLabel')}>
        <input
          type="number"
          min={0}
          max={60}
          value={prefs.polishContextWindowMinutes}
          onChange={e => onPolishContextWindowChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
    </Card>
  );
}
