import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../components/ShortcutRecorder';
import { isDesktop, setQuickNoteHotkey } from '../lib/ipc';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card } from './_atoms';
import { History } from './History';

const QUICK_NOTE_SHORTCUT_HIDDEN_KEY = 'openless.quick-note.shortcut-hidden';

/** Quick notes share the unified history/actions surface but use permanent audio retention. */
export function QuickNote() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [shortcutVisible, setShortcutVisible] = useState(() => {
    try {
      return window.localStorage.getItem(QUICK_NOTE_SHORTCUT_HIDDEN_KEY) !== '1';
    } catch {
      return true;
    }
  });

  const hideShortcut = () => {
    setShortcutVisible(false);
    try {
      window.localStorage.setItem(QUICK_NOTE_SHORTCUT_HIDDEN_KEY, '1');
    } catch {
      // The layout preference is best-effort when localStorage is unavailable.
    }
  };

  const showShortcut = () => {
    setShortcutVisible(true);
    try {
      window.localStorage.removeItem(QUICK_NOTE_SHORTCUT_HIDDEN_KEY);
    } catch {
      // The layout preference is best-effort when localStorage is unavailable.
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12, height: '100%', minHeight: 0 }}>
      {isDesktop() && shortcutVisible && (
        <Card>
          <div
            style={{
              display: 'flex',
              alignItems: 'flex-start',
              justifyContent: 'space-between',
              gap: 12,
              marginBottom: 6,
            }}
          >
            <div style={{ fontSize: 13, fontWeight: 600 }}>
              {t('quickNote.shortcutTitle', 'Quick note shortcut')}
            </div>
            <Btn
              icon="x"
              variant="ghost"
              size="sm"
              ariaLabel={t('common.hide', '隐藏')}
              title={t('common.hide', '隐藏')}
              onClick={hideShortcut}
              style={{ padding: '4px 7px', marginTop: -4, marginRight: -4 }}
            />
          </div>
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginBottom: 10 }}>
            {t(
              'quickNote.shortcutDesc',
              'Press once to start a permanent audio capture, then press again to finish.',
            )}
          </div>
          {prefs && (
            <ShortcutRecorder
              value={prefs.quickNoteHotkey}
              onSave={async (binding) => {
                await setQuickNoteHotkey(binding);
                await updatePrefs({ ...prefs, quickNoteHotkey: binding });
              }}
              onDisable={async () => {
                await setQuickNoteHotkey(null);
                await updatePrefs({ ...prefs, quickNoteHotkey: null });
              }}
            />
          )}
        </Card>
      )}
      {isDesktop() && !shortcutVisible && (
        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <Btn icon="bolt" variant="ghost" size="sm" onClick={showShortcut}>
            {t('quickNote.showShortcut', '显示速记快捷键')}
          </Btn>
        </div>
      )}
      <div style={{ flex: 1, minHeight: 0 }}>
        <History quickNotesOnly />
      </div>
    </div>
  );
}
