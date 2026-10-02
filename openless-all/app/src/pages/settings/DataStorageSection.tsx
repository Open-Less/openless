// Retention, cleanup, and storage settings for history and context data.

import { useTranslation } from 'react-i18next';
import { detectOS } from '../../components/WindowChrome';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle, inputStyle } from './shared';

// Range limits: retention 0-365 days, context window 0-60 minutes (larger values are meaningless
// for real conversation scenarios and just burn tokens).
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

  // Empty string rolls back to the default.
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
  // History cap 200 is the current HISTORY_CAP (persistence.rs:32); the lower bound of 5 keeps a
  // user-entered 0 from wiping everything as soon as one entry is written; empty string means
  // unlimited, falling back to null → the backend applies its 200 default.
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
          onChange={(e) => onHistoryRetentionChange(e.target.value)}
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
          onChange={(e) => onHistoryMaxEntriesChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      <SettingRow label={t('settings.recording.polishContextWindowLabel')}>
        <input
          type="number"
          min={0}
          max={60}
          value={prefs.polishContextWindowMinutes}
          onChange={(e) => onPolishContextWindowChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      {/* Cursor context. Placing it under "Privacy" rather than "Polish" is deliberate: this
          toggle's real cost is not tokens but "sending text from other apps to the LLM provider".
          Shown on macOS and Windows — Linux has no implementation, and a switch that changes
          nothing would just mislead. */}
      {(detectOS() === 'mac' || detectOS() === 'win') && (
        <SettingRow
          label={t('settings.dataStorage.cursorContextLabel')}
          desc={t('settings.dataStorage.cursorContextDesc')}
        >
          <Toggle
            on={prefs.cursorContextEnabled}
            onToggle={(next) => void savePrefs({ ...prefs, cursorContextEnabled: next })}
          />
        </SettingRow>
      )}
    </Card>
  );
}
