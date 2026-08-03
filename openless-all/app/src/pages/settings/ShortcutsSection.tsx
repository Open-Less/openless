// 快捷键设置：开始/停止、翻译、问答、切风格、唤起 App、以及只读取消/确认提示。

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import {
  defaultLessComputerShortcut,
  defaultOpenAppShortcut,
  defaultQaShortcut,
  defaultSwitchStyleShortcut,
  hotkeyModeSuffix,
} from '../../lib/hotkey';
import {
  setDictationHotkey,
  setOpenAppHotkey,
  setQaHotkey,
  setSwitchStyleHotkey,
  setTranslationHotkey,
} from '../../lib/ipc';
import { getPlatformCapabilities } from '../../lib/platform';
import type { PlatformCapabilities } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { SettingRow } from './shared';
import { detectOS } from '../../components/WindowChrome';

export function ShortcutsSection() {
  const { t } = useTranslation();
  const os = detectOS();
  const { prefs, hotkey, updatePrefs: savePrefs } = useHotkeySettings();
  const [platformCaps, setPlatformCaps] = useState<PlatformCapabilities | null>(null);

  useEffect(() => {
    void getPlatformCapabilities().then(setPlatformCaps);
  }, []);

  if (!prefs || !hotkey) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  if (platformCaps && !platformCaps.supportsDesktopHotkey) {
    return null;
  }

  const readonlyRows: Array<[string, string]> = [
    [t('settings.shortcuts.cancel'), 'Esc'],
    // 胶囊右侧「✓ 确认插入」目前只在 macOS 胶囊上有，Windows/Linux 胶囊没有这个按钮，
    // 之前 os !== 'linux' 把它也展示给了 Windows，误导用户以为有个用不了的快捷键（issue #780）。
    ...(os === 'mac' ? [[t('settings.shortcuts.confirm'), t('settings.shortcuts.confirmHint')]] as Array<[string, string]> : []),
  ];
  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 6 }}>{t('settings.shortcuts.title')}</div>
      <SettingRow label={t('settings.shortcuts.startStop')}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6, width: '100%' }}>
          <ShortcutRecorder
            value={prefs.dictationHotkey}
            sideSpecificModifiers
            // 与「录音与输入」页一致：核心热键不可停用，置灰并提示。
            disableDisabled
            disableHint={t('settings.recording.comboDisableHint')}
            onSave={async binding => {
              await setDictationHotkey(binding);
              await savePrefs({ ...prefs, dictationHotkey: binding });
            }}
          />
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {hotkeyModeSuffix(hotkey.mode)}
          </div>
        </div>
      </SettingRow>
      <SettingRow label={t('translation.hotkey.title', 'Translation shortcut')}>
        <ShortcutRecorder
          value={prefs.translationHotkey}
          onSave={async binding => {
            await setTranslationHotkey(binding);
            await savePrefs({ ...prefs, translationHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('selectionAsk.hotkey.title')}>
        <ShortcutRecorder
          value={prefs.qaHotkey}
          onSave={async binding => {
            await setQaHotkey(binding);
            await savePrefs({ ...prefs, qaHotkey: binding });
          }}
          onDisable={async () => {
            await setQaHotkey(null);
            await savePrefs({ ...prefs, qaHotkey: null });
          }}
          onReset={async () => {
            const binding = defaultQaShortcut();
            await setQaHotkey(binding);
            await savePrefs({ ...prefs, qaHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('settings.shortcuts.switchStyle')}>
        <ShortcutRecorder
          value={prefs.switchStyleHotkey}
          onSave={async binding => {
            await setSwitchStyleHotkey(binding);
            await savePrefs({ ...prefs, switchStyleHotkey: binding });
          }}
          onDisable={async () => {
            await setSwitchStyleHotkey(null);
            await savePrefs({ ...prefs, switchStyleHotkey: null });
          }}
          onReset={async () => {
            const binding = defaultSwitchStyleShortcut();
            await setSwitchStyleHotkey(binding);
            await savePrefs({ ...prefs, switchStyleHotkey: binding });
          }}
        />
      </SettingRow>
      <SettingRow label={t('settings.shortcuts.openApp')}>
        <ShortcutRecorder
          value={prefs.openAppHotkey}
          onSave={async binding => {
            await setOpenAppHotkey(binding);
            await savePrefs({ ...prefs, openAppHotkey: binding });
          }}
          onDisable={async () => {
            await setOpenAppHotkey(null);
            await savePrefs({ ...prefs, openAppHotkey: null });
          }}
          onReset={async () => {
            const binding = defaultOpenAppShortcut();
            await setOpenAppHotkey(binding);
            await savePrefs({ ...prefs, openAppHotkey: binding });
          }}
        />
      </SettingRow>
      {os === 'mac' && (
        <SettingRow label={t('settings.codingAgent.title')} desc={t('settings.codingAgent.voiceHotkeyDesc')}>
          <ShortcutRecorder
            value={prefs.codingAgentVoiceHotkey}
            onSave={async binding => {
              await savePrefs({ ...prefs, codingAgentEnabled: true, codingAgentVoiceHotkey: binding });
            }}
            onDisable={async () => {
              await savePrefs({ ...prefs, codingAgentVoiceHotkey: null });
            }}
            onReset={async () => {
              await savePrefs({ ...prefs, codingAgentEnabled: true, codingAgentVoiceHotkey: defaultLessComputerShortcut() });
            }}
          />
        </SettingRow>
      )}
      {readonlyRows.map(([k, v]) => (
        <SettingRow key={k} label={k}>
          <kbd style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
            borderRadius: 6, background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line-strong)',
            boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
            color: 'var(--ol-ink-2)',
          }}>{v}</kbd>
        </SettingRow>
      ))}
    </Card>
  );
}
