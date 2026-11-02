// General → Selection Assistant: merges selection polish and selection voice editing so users don't confuse the two jobs.

import type { PlatformCapabilities, SelectionPolishOutputMode } from '../../lib/types';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { defaultSelectionPolishShortcut, getHotkeyStartStopLabel } from '../../lib/hotkey';
import { openVoiceEditWindow, setSelectionPolishHotkey } from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Segmented } from '../../components/ui/Segmented';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow, Toggle } from './shared';
import { SelectionWorkspaceExtraControlsDialog } from './SelectionWorkspaceExtraControlsDialog';

const outputOptions: Array<{ value: SelectionPolishOutputMode }> = [
  { value: 'directReplace' },
  { value: 'previewConfirm' },
];

export function SelectionWorkspaceSection({ showShortcut = true }: { showShortcut?: boolean }) {
  const { t } = useTranslation();
  const { prefs, capability, refresh, updatePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);
  const [extraControlsOpen, setExtraControlsOpen] = useState(false);

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
                label={t('settings.selectionWorkspace.extraControls')}
                desc={t('settings.selectionWorkspace.extraControlsDesc')}
              >
                <button
                  type="button"
                  onClick={() => setExtraControlsOpen(true)}
                  aria-haspopup="dialog"
                  style={{
                    border: '0.5px solid var(--ol-line-strong)',
                    borderRadius: 7,
                    background: 'var(--ol-control-solid)',
                    color: 'var(--ol-ink-2)',
                    padding: '7px 12px',
                    fontSize: 12,
                    fontFamily: 'inherit',
                    cursor: 'pointer',
                  }}
                >
                  {t('settings.selectionWorkspace.openExtraControls')}
                </button>
              </SettingRow>
            </>
          )}
        </>
      )}
      <SelectionWorkspaceExtraControlsDialog
        open={extraControlsOpen}
        autoIntent={autoIntent}
        keywordsText={keywordsText}
        editPlanFormat={editPlanFormat}
        editSystemPrompt={editSystemPrompt}
        updatePrefs={updatePrefs}
        onClose={() => setExtraControlsOpen(false)}
      />
    </Card>
  );
}
