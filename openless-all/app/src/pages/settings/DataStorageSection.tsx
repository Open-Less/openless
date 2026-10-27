// Retention, cleanup, and storage settings for history and context data.

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS } from '../../components/WindowChrome';
import { copyTextToClipboard } from '../../lib/clipboard';
import { buildPolishStrategyAgentSkill } from '../../lib/polishStrategyAgentSkill';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle, btnGhostStyle, inputStyle } from './shared';

// Range limits: retention 0-365 days, context window 0-60 minutes (larger values are meaningless
// for real conversation scenarios and just burn tokens).
const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export function DataStorageSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();
  const [strategyCopyState, setStrategyCopyState] = useState<'idle' | 'copied' | 'failed'>('idle');

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
  // Issue #1032: 0 = off. Empty string rolls back to off. Upper bound 50 characters keeps the
  // skip limited to short fragments; longer dictations still get polished.
  const onPolishSkipMaxCharsChange = (raw: string) => {
    const parsed = raw === '' ? 0 : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return;
    void savePrefs({ ...prefs, polishSkipMaxChars: clamp(parsed, 0, 50) });
  };
  const onPolishStrategyScriptChange = (raw: string) => {
    void savePrefs({ ...prefs, polishStrategyScript: raw.slice(0, 32_768) });
  };
  const copyPolishStrategySkill = async () => {
    const copied = await copyTextToClipboard(
      buildPolishStrategyAgentSkill(prefs.polishStrategyScript),
    );
    setStrategyCopyState(copied ? 'copied' : 'failed');
    window.setTimeout(() => setStrategyCopyState('idle'), 1500);
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
      <SettingRow
        label={t('settings.recording.polishSkipMaxCharsLabel')}
        desc={t('settings.recording.polishSkipMaxCharsDesc')}
      >
        <input
          type="number"
          min={0}
          max={50}
          value={prefs.polishSkipMaxChars}
          onChange={(e) => onPolishSkipMaxCharsChange(e.target.value)}
          style={{ ...inputStyle, width: 80, textAlign: 'right' }}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.recording.polishStrategyEnabledLabel')}
        desc={t('settings.recording.polishStrategyEnabledDesc')}
      >
        <Toggle
          on={prefs.polishStrategyEnabled}
          onToggle={(next) => void savePrefs({ ...prefs, polishStrategyEnabled: next })}
        />
      </SettingRow>
      <SettingRow
        label={t('settings.recording.polishStrategyScriptLabel')}
        desc={t('settings.recording.polishStrategyScriptDesc')}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'flex-start',
            gap: 8,
            flex: '1 1 520px',
            minWidth: 0,
            maxWidth: '100%',
            flexWrap: 'wrap',
          }}
        >
          <textarea
            value={prefs.polishStrategyScript}
            onChange={(event) => onPolishStrategyScriptChange(event.target.value)}
            placeholder={t('settings.recording.polishStrategyScriptPlaceholder')}
            spellCheck={false}
            rows={8}
            maxLength={32_768}
            style={{
              ...inputStyle,
              flex: '1 1 360px',
              minWidth: 0,
              width: 'auto',
              height: 150,
              resize: 'vertical',
              fontFamily: 'monospace',
            }}
          />
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: 6,
              alignItems: 'flex-start',
            }}
          >
            <button
              type="button"
              style={{ ...btnGhostStyle, cursor: 'pointer', whiteSpace: 'nowrap' }}
              onClick={() => void copyPolishStrategySkill()}
            >
              {strategyCopyState === 'copied'
                ? t('settings.recording.polishStrategySkillCopied')
                : t('settings.recording.polishStrategySkillCopy')}
            </button>
            {strategyCopyState !== 'idle' && (
              <span
                role={strategyCopyState === 'failed' ? 'alert' : 'status'}
                style={{
                  fontSize: 11,
                  color: strategyCopyState === 'failed' ? 'var(--ol-err)' : 'var(--ol-ok)',
                }}
              >
                {strategyCopyState === 'failed'
                  ? t('settings.recording.polishStrategySkillCopyFailed')
                  : t('settings.recording.polishStrategySkillCopied')}
              </span>
            )}
          </div>
        </div>
      </SettingRow>
      {/* Cursor context. Placing it under "Privacy" rather than "Polish" is deliberate: this
          toggle's real cost is not tokens but "sending text from other apps to the LLM provider".
          Shown on macOS, Windows and Android — Linux has no implementation. */}
      {(detectOS() === 'mac' || detectOS() === 'win' || detectOS() === 'android') && (
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
