// Retention, cleanup, and storage settings for history and context data.

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useHotkeySettings } from '../../../data/preferences';
import { Card } from '../../../ui/layout/Card';
import { SettingRow, Toggle } from '../../../ui/controls/SettingRow';
import { SectionTitle } from '../../../ui/layout/SectionTitle';
import { btnGhostStyle, inputStyle } from '../../../ui/controls/settingsFieldStyles';
import { copyTextToClipboard } from '../../../api/files';
import { useCopyFeedback } from '../../../ui/feedback/useCopyFeedback';
import { buildPolishStrategyAgentSkill } from '../../../lib/polishStrategyAgentSkill';

// Range limits: retention 0-365 days, context window 0-60 minutes (larger values are meaningless
// for real conversation scenarios and just burn tokens).
const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));

export function DataStorageSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs: savePrefs } = useHotkeySettings();

  const [copyFailed, setCopyFailed] = useState(false);
  const { copied, copy } = useCopyFeedback(copyTextToClipboard, () => setCopyFailed(true));

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
      <SettingRow
        label={t('settings.recording.polishStrategyEnabledLabel')}
        desc={t('settings.recording.polishStrategyEnabledDesc')}
      >
        <Toggle
          on={prefs.polishStrategyEnabled}
          onToggle={(next) =>
            void savePrefs((current) => ({ ...current, polishStrategyEnabled: next }))
          }
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
            aria-label={t('settings.recording.polishStrategyScriptLabel')}
            onChange={(event) => {
              const polishStrategyScript = event.target.value.slice(0, 32_768);
              void savePrefs((current) => ({ ...current, polishStrategyScript }));
            }}
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
              onClick={() => {
                setCopyFailed(false);
                void copy('strategy', buildPolishStrategyAgentSkill(prefs.polishStrategyScript));
              }}
            >
              {copied === 'strategy'
                ? t('settings.recording.polishStrategySkillCopied')
                : t('settings.recording.polishStrategySkillCopy')}
            </button>
            {(copyFailed || copied) && (
              <span
                role={copyFailed ? 'alert' : 'status'}
                style={{
                  fontSize: 11,
                  color: copyFailed ? 'var(--ol-err)' : 'var(--ol-ok)',
                }}
              >
                {copyFailed
                  ? t('settings.recording.polishStrategySkillCopyFailed')
                  : t('settings.recording.polishStrategySkillCopied')}
              </span>
            )}
          </div>
        </div>
      </SettingRow>
    </Card>
  );
}
