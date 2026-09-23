// 通用 → 选区助手：合并选区润色与选区语音编辑，避免用户混淆两项职责。

import type {
  EditPlanFormat,
  PlatformCapabilities,
  SelectionPolishOutputMode,
} from '../../lib/types';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { detectOS } from '../../components/WindowChrome';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { defaultSelectionPolishShortcut, getHotkeyStartStopLabel } from '../../lib/hotkey';
import { setSelectionPolishHotkey } from '../../lib/ipc';
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

const outputOptions: Array<{ value: SelectionPolishOutputMode }> = [
  { value: 'directReplace' },
  { value: 'previewConfirm' },
];

const editPlanFormatOptions: Array<{ value: EditPlanFormat }> = [
  { value: 'xml' },
  { value: 'json' },
];

export function SelectionWorkspaceSection() {
  const { t } = useTranslation();
  const { prefs, capability, refresh, updatePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const os = detectOS();

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  if (!prefs || !capability || !platformCaps?.supportsDesktopHotkey) return null;

  const recordingLabel = getHotkeyStartStopLabel(
    prefs.hotkey,
    prefs.customComboHotkey,
    prefs.dictationHotkey,
  );
  const autoIntent = prefs.selectionVoiceIntentMode === 'auto';
  const keywordsText = prefs.selectionVoiceEditKeywords.join('\n');
  const showVoice = os === 'win';
  const voiceEnabled = prefs.selectionVoiceEnabled;
  const editPlanFormat = prefs.selectionVoiceEditPlanFormat ?? 'xml';
  const editSystemPrompt = prefs.selectionVoiceEditSystemPrompt ?? '';

  return (
    <Card>
      <SectionTitle hint={t('settings.selectionWorkspace.hint')}>
        {t('settings.selectionWorkspace.title')}
      </SectionTitle>

      <SettingRow
        label={t('settings.selectionWorkspace.polishHotkey')}
        desc={t('settings.selectionWorkspace.polishHotkeyDesc')}
      >
        <ShortcutRecorder
          value={prefs.selectionPolishHotkey}
          onSave={async (binding) => {
            await setSelectionPolishHotkey(binding);
            await refresh();
          }}
          onDisable={async () => {
            await setSelectionPolishHotkey(null);
            await refresh();
          }}
          onReset={async () => {
            await setSelectionPolishHotkey(defaultSelectionPolishShortcut());
            await refresh();
          }}
        />
      </SettingRow>
      {!voiceEnabled && (
        <SettingRow label={t('settings.selectionWorkspace.polishDelivery')}>
          <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
            {outputOptions.map((option) => {
              const selected = prefs.selectionPolishOutputMode === option.value;
              return (
                <button
                  key={option.value}
                  title={t(`settings.selectionPolish.${option.value}Hint`)}
                  onClick={() =>
                    void updatePrefs((current) => ({
                      ...current,
                      selectionPolishOutputMode: option.value,
                    }))
                  }
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
                  {t(`settings.selectionPolish.${option.value}`)}
                </button>
              );
            })}
          </div>
        </SettingRow>
      )}

      {showVoice && (
        <>
          <SettingRow
            label={t('settings.selectionWorkspace.voiceEnable')}
            desc={t('settings.selectionWorkspace.voiceEnableDesc', { recordingLabel })}
          >
            <Toggle
              on={voiceEnabled}
              onToggle={(next) =>
                void updatePrefs((current) => ({ ...current, selectionVoiceEnabled: next }))
              }
            />
          </SettingRow>
          {voiceEnabled && (
            <>
              <SettingRow
                label={t('settings.selectionWorkspace.polishDelivery')}
                desc={t('settings.selectionWorkspace.voiceDeliveryDesc')}
              >
                <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
                  {outputOptions.map((option) => {
                    const selected = prefs.selectionPolishOutputMode === option.value;
                    return (
                      <button
                        key={option.value}
                        title={t(`settings.selectionPolish.${option.value}Hint`)}
                        onClick={() =>
                          void updatePrefs((current) => ({
                            ...current,
                            selectionPolishOutputMode: option.value,
                          }))
                        }
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
                        {t(`settings.selectionPolish.${option.value}`)}
                      </button>
                    );
                  })}
                </div>
              </SettingRow>
              <SettingRow
                label={t('settings.selectionWorkspace.autoIntent')}
                desc={t('settings.selectionWorkspace.autoIntentDesc')}
              >
                <Toggle
                  on={autoIntent}
                  onToggle={(next) =>
                    void updatePrefs((current) => ({
                      ...current,
                      selectionVoiceIntentMode: next ? 'auto' : 'heuristic',
                    }))
                  }
                />
              </SettingRow>
              {!autoIntent && (
                <SettingRow
                  label={t('settings.selectionWorkspace.editKeywords')}
                  desc={t('settings.selectionWorkspace.editKeywordsDesc')}
                >
                  <textarea
                    aria-label={t('settings.selectionWorkspace.editKeywords')}
                    value={keywordsText}
                    onChange={(event) => {
                      const lines = event.target.value
                        .split(/\n/)
                        .map((line) => line.trim())
                        .filter(Boolean);
                      void updatePrefs((current) => ({
                        ...current,
                        selectionVoiceEditKeywords: lines,
                        selectionVoiceIntentMode: 'heuristic',
                      }));
                    }}
                    rows={6}
                    style={{
                      ...inputStyle,
                      width: '100%',
                      minWidth: 220,
                      minHeight: 120,
                      resize: 'vertical',
                      lineHeight: 1.5,
                      fontFamily: 'inherit',
                    }}
                  />
                </SettingRow>
              )}
              <SettingRow
                label={t('settings.selectionWorkspace.editPlanFormat')}
                desc={t('settings.selectionWorkspace.editPlanFormatDesc')}
              >
                <div style={{ ...segmentedTrackStyle, flexWrap: 'wrap', gap: 4 }}>
                  {editPlanFormatOptions.map((option) => {
                    const selected = editPlanFormat === option.value;
                    return (
                      <button
                        key={option.value}
                        type="button"
                        onClick={() =>
                          void updatePrefs((current) => ({
                            ...current,
                            selectionVoiceEditPlanFormat: option.value,
                          }))
                        }
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
                        {t(
                          `settings.selectionWorkspace.editPlanFormat${option.value === 'xml' ? 'Xml' : 'Json'}`,
                        )}
                      </button>
                    );
                  })}
                </div>
              </SettingRow>
              <SettingRow
                label={t('settings.selectionWorkspace.editSystemPrompt')}
                desc={t('settings.selectionWorkspace.editSystemPromptDesc')}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%' }}>
                  <textarea
                    aria-label={t('settings.selectionWorkspace.editSystemPrompt')}
                    value={editSystemPrompt}
                    placeholder={t('settings.selectionWorkspace.editSystemPromptPlaceholder')}
                    onChange={(event) => {
                      const next = event.target.value;
                      void updatePrefs((current) => ({
                        ...current,
                        selectionVoiceEditSystemPrompt: next,
                      }));
                    }}
                    rows={8}
                    style={{
                      ...inputStyle,
                      width: '100%',
                      minWidth: 220,
                      minHeight: 140,
                      resize: 'vertical',
                      lineHeight: 1.5,
                      fontFamily: 'inherit',
                    }}
                  />
                  <button
                    type="button"
                    onClick={() =>
                      void updatePrefs((current) => ({
                        ...current,
                        selectionVoiceEditSystemPrompt: '',
                      }))
                    }
                    style={{
                      alignSelf: 'flex-start',
                      border: '0.5px solid var(--ol-line)',
                      borderRadius: 6,
                      background: 'transparent',
                      color: 'var(--ol-ink-3)',
                      padding: '4px 10px',
                      fontSize: 12,
                      fontFamily: 'inherit',
                      cursor: 'default',
                    }}
                  >
                    {t('settings.selectionWorkspace.editSystemPromptReset')}
                  </button>
                </div>
              </SettingRow>
            </>
          )}
        </>
      )}
    </Card>
  );
}
