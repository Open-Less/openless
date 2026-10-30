// General → Selection Assistant: merges selection polish and selection voice editing so users don't confuse the two jobs.

import type {
  EditPlanFormat,
  PlatformCapabilities,
  SelectionPolishOutputMode,
} from '../../lib/types';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { defaultSelectionPolishShortcut, getHotkeyStartStopLabel } from '../../lib/hotkey';
import { openVoiceEditWindow, setSelectionPolishHotkey } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Segmented } from '../../components/ui/Segmented';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle, inputStyle } from './shared';

const outputOptions: Array<{ value: SelectionPolishOutputMode }> = [
  { value: 'directReplace' },
  { value: 'previewConfirm' },
];

const editPlanFormatOptions: Array<{ value: EditPlanFormat }> = [
  { value: 'xml' },
  { value: 'json' },
];

export function SelectionWorkspaceSection({ showShortcut = true }: { showShortcut?: boolean }) {
  const { t } = useTranslation();
  const { prefs, capability, refresh, updatePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  if (
    !prefs ||
    !capability ||
    (!platformCaps?.supportsDesktopHotkey && !platformCaps?.supportsVoiceEdit)
  )
    return null;

  const recordingLabel = getHotkeyStartStopLabel(
    prefs.hotkey,
    prefs.customComboHotkey,
    prefs.dictationHotkey,
  );
  const autoIntent = prefs.selectionVoiceIntentMode === 'auto';
  const keywordsText = prefs.selectionVoiceEditKeywords.join('\n');
  const showDesktop = platformCaps.supportsDesktopHotkey;
  const showVoice = platformCaps.supportsVoiceEdit;
  const voiceEnabled = prefs.selectionVoiceEnabled;
  const editPlanFormat = prefs.selectionVoiceEditPlanFormat ?? 'xml';
  const editSystemPrompt = prefs.selectionVoiceEditSystemPrompt ?? '';
  const deliveryControl = (
    <Segmented
      label={t('settings.selectionWorkspace.polishDelivery')}
      value={prefs.selectionPolishOutputMode}
      onChange={(next) =>
        void updatePrefs((current) => ({ ...current, selectionPolishOutputMode: next }))
      }
      options={outputOptions.map((option) => ({
        value: option.value,
        label: t(`settings.selectionPolish.${option.value}`),
        title: t(`settings.selectionPolish.${option.value}Hint`),
      }))}
    />
  );

  return (
    <Card>
      <SectionTitle hint={t('settings.selectionWorkspace.hint')}>
        {t('settings.selectionWorkspace.title')}
      </SectionTitle>

      {showVoice && (
        <SettingRow label={t('voiceEdit.title')} desc={t('voiceEdit.description')}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <Toggle
              on={prefs.voiceEditEnabled}
              onToggle={(next) =>
                void updatePrefs((current) => ({ ...current, voiceEditEnabled: next }))
              }
            />
            <button
              type="button"
              disabled={!prefs.voiceEditEnabled}
              onClick={() => void openVoiceEditWindow()}
              style={{
                background: 'transparent',
                border: '0.5px solid var(--ol-line)',
                color: 'var(--ol-ink-3)',
                borderRadius: 6,
                padding: '6px 10px',
                fontFamily: 'inherit',
                fontSize: 12,
                cursor: prefs.voiceEditEnabled ? 'pointer' : 'not-allowed',
                opacity: prefs.voiceEditEnabled ? 1 : 0.5,
              }}
            >
              {t('voiceEdit.openPanel')}
            </button>
          </div>
        </SettingRow>
      )}
      {showDesktop && showShortcut && (
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
      )}
      {showDesktop && !voiceEnabled && (
        <SettingRow label={t('settings.selectionWorkspace.polishDelivery')}>
          {deliveryControl}
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
                {deliveryControl}
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
                <Segmented
                  label={t('settings.selectionWorkspace.editPlanFormat')}
                  value={editPlanFormat}
                  onChange={(next) =>
                    void updatePrefs((current) => ({
                      ...current,
                      selectionVoiceEditPlanFormat: next,
                    }))
                  }
                  options={editPlanFormatOptions.map((option) => ({
                    value: option.value,
                    label: t(
                      `settings.selectionWorkspace.editPlanFormat${option.value === 'xml' ? 'Xml' : 'Json'}`,
                    ),
                  }))}
                />
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
