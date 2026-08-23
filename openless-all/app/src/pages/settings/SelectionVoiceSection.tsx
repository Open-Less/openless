// 通用 → 选区语音编辑（issue #987 Windows MVP）：选中文字 + 专用快捷键 + 语音指令。

import type {
  PlatformCapabilities,
  SelectionVoiceIntentMode,
  SelectionVoiceManualIntent,
} from '../../lib/types';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS } from '../../components/WindowChrome';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import {
  defaultSelectionVoiceShortcut,
  getHotkeyStartStopLabel,
} from '../../lib/hotkey';
import { setSelectionVoiceHotkey } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import {
  SectionTitle,
  SettingRow,
  Toggle,
  chipSelectedStyle,
  inputStyle,
  segmentedTrackStyle,
} from './shared';

const intentModeOptions: SelectionVoiceIntentMode[] = ['auto', 'manual', 'heuristic'];
const manualIntentOptions: SelectionVoiceManualIntent[] = ['question', 'edit'];

export function SelectionVoiceSection() {
  const { t } = useTranslation();
  const { prefs, capability, refresh, updatePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const os = detectOS();

  useEffect(() => { void getPlatformCapabilities().then(setPlatformCaps); }, []);

  if (
    os !== 'win'
    || !prefs
    || !capability
    || !platformCaps?.supportsDesktopHotkey
  ) {
    return null;
  }

  const recordingLabel = getHotkeyStartStopLabel(
    prefs.hotkey,
    prefs.customComboHotkey,
    prefs.dictationHotkey,
  );

  const keywordsText = prefs.selectionVoiceEditKeywords.join('\n');

  return (
    <Card>
      <SectionTitle hint={t('settings.selectionVoice.hint')}>
        {t('settings.selectionVoice.title')}
      </SectionTitle>
      <SettingRow
        label={t('settings.selectionVoice.enable')}
        desc={t('settings.selectionVoice.enableDesc', { recordingLabel })}
      >
        <Toggle
          on={prefs.selectionVoiceEnabled}
          onToggle={next => void updatePrefs(current => ({ ...current, selectionVoiceEnabled: next }))}
        />
      </SettingRow>
      {prefs.selectionVoiceEnabled && (
        <>
          <SettingRow
            label={t('settings.selectionVoice.hotkey')}
            desc={t('settings.selectionVoice.hotkeyDesc')}
          >
            <ShortcutRecorder
              value={prefs.selectionVoiceHotkey}
              onSave={async binding => {
                await setSelectionVoiceHotkey(binding);
                await refresh();
              }}
              onDisable={async () => {
                await setSelectionVoiceHotkey(null);
                await refresh();
              }}
              onReset={async () => {
                await setSelectionVoiceHotkey(defaultSelectionVoiceShortcut());
                await refresh();
              }}
            />
          </SettingRow>
          <SettingRow label={t('settings.selectionVoice.intentMode')}>
            <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
              {intentModeOptions.map(option => {
                const selected = prefs.selectionVoiceIntentMode === option;
                return (
                  <button
                    key={option}
                    title={t(`settings.selectionVoice.intentMode.${option}Hint`)}
                    onClick={() => void updatePrefs(current => ({
                      ...current,
                      selectionVoiceIntentMode: option,
                    }))}
                    style={{
                      ...chipSelectedStyle(selected),
                      border: 0,
                      borderRadius: 6,
                      padding: '6px 10px',
                      fontFamily: 'inherit',
                      fontSize: 12,
                      cursor: 'default',
                      fontWeight: selected ? 600 : 500,
                    }}
                  >
                    {t(`settings.selectionVoice.intentMode.${option}`)}
                  </button>
                );
              })}
            </div>
          </SettingRow>
          {prefs.selectionVoiceIntentMode === 'manual' && (
            <SettingRow label={t('settings.selectionVoice.manualIntent')}>
              <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
                {manualIntentOptions.map(option => {
                  const selected = prefs.selectionVoiceManualIntent === option;
                  return (
                    <button
                      key={option}
                      title={t(`settings.selectionVoice.manualIntent.${option}Hint`)}
                      onClick={() => void updatePrefs(current => ({
                        ...current,
                        selectionVoiceManualIntent: option,
                      }))}
                      style={{
                        ...chipSelectedStyle(selected),
                        border: 0,
                        borderRadius: 6,
                        padding: '6px 10px',
                        fontFamily: 'inherit',
                        fontSize: 12,
                        cursor: 'default',
                        fontWeight: selected ? 600 : 500,
                      }}
                    >
                      {t(`settings.selectionVoice.manualIntent.${option}`)}
                    </button>
                  );
                })}
              </div>
            </SettingRow>
          )}
          {prefs.selectionVoiceIntentMode === 'heuristic' && (
            <SettingRow
              label={t('settings.selectionVoice.editKeywords')}
              desc={t('settings.selectionVoice.editKeywordsDesc')}
            >
              <textarea
                aria-label={t('settings.selectionVoice.editKeywords')}
                value={keywordsText}
                onChange={event => {
                  const lines = event.target.value
                    .split(/\n/)
                    .map(line => line.trim())
                    .filter(Boolean);
                  void updatePrefs(current => ({
                    ...current,
                    selectionVoiceEditKeywords: lines,
                  }));
                }}
                rows={4}
                style={{
                  ...inputStyle,
                  width: '100%',
                  minWidth: 220,
                  resize: 'vertical',
                  lineHeight: 1.5,
                  fontFamily: 'inherit',
                }}
              />
            </SettingRow>
          )}
        </>
      )}
    </Card>
  );
}
