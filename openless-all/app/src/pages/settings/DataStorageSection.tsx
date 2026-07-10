// 隐私 → 数据存储：本地保留的历史会话与对话上下文窗口。
// 自 Settings.tsx 的 RecordingSection「历史与上下文」折叠组拆出，逻辑零改动。

import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, inputStyle } from './shared';

// 范围限制：retention 0-365 天，context window 0-60 分钟（再大对实际对话场景没意义且白烧 token）。
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

  // 空字符串时回滚到默认值。
  const onHistoryRetentionChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, historyRetentionDays: clamp(parsed, 0, 365) });
  };
  const onPolishContextWindowChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, polishContextWindowMinutes: clamp(parsed, 0, 60) });
  };
  // 历史条数 200 是当前 HISTORY_CAP（persistence.rs:32），下限 5 是避免用户填 0 导致
  // 写一条就立刻被清光；空字符串视为不限制，落回 null → 后端走 200 默认。
  const onHistoryMaxEntriesChange = (raw: string) => {
    const trimmed = raw.trim();
    if (trimmed === '') {
      void savePrefs({ ...prefs, historyMaxEntries: null });
      return;
    }
    const parsed = Number.parseInt(trimmed, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, historyMaxEntries: clamp(parsed, 5, 200) });
  };

  return (
    <Card>
      <SectionTitle>{t('settings.dataStorage.title')}</SectionTitle>
      <SettingRow label={t('settings.recording.historyRetentionLabel')}>
        <input
          type="number"
          min={0}
          max={365}
          value={prefs.historyRetentionDays}
          onChange={e => onHistoryRetentionChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      <SettingRow label={t('settings.recording.historyMaxEntriesLabel')}>
        <input
          type="number"
          min={5}
          max={200}
          placeholder="200"
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
