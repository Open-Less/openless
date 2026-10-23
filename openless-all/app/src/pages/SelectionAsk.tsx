import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PageHeader } from './_atoms';
import './SelectionAsk.css';
import { ShortcutRecorder } from '../components/ShortcutRecorder';
import { Toggle } from './settings/shared';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { setQaHotkey } from '../lib/ipc';
import { formatComboLabel } from '../lib/hotkey';

export function SelectionAsk({ onOpenShortcuts }: { onOpenShortcuts?: () => void }) {
  const { t } = useTranslation();
  const { prefs, refresh, updatePrefs } = useHotkeySettings();
  const [error, setError] = useState('');
  if (!prefs) return <p>{t('common.loading')}</p>;
  return (
    <div style={{ width: '100%', maxWidth: 960, margin: '0 auto' }}>
      <PageHeader title={t('askAnything.question')} desc={t('askAnything.description')} />
      <div className="ol-ask-settings-row">
        <div>
          <strong>{t('askAnything.auxiliaryKey')}</strong>
          <p>{t('askAnything.auxiliaryHint', { key: formatComboLabel(prefs.dictationHotkey) })}</p>
        </div>
        <ShortcutRecorder
          value={prefs.qaCompanionKey ? { primary: prefs.qaCompanionKey, modifiers: [] } : null}
          onSave={async (binding) => {
            await setQaHotkey(binding);
            await refresh();
          }}
          onDisable={async () => {
            await setQaHotkey(null);
            await refresh();
          }}
          onReset={async () => {
            await setQaHotkey({ primary: 'Space', modifiers: [] });
            await refresh();
          }}
        />
      </div>
      <div className="ol-ask-settings-row">
        <div>
          <strong>{t('selectionAsk.history.title')}</strong>
          <p>{t('selectionAsk.history.desc')}</p>
        </div>
        <Toggle
          on={prefs.qaSaveHistory}
          onToggle={(next) => {
            setError('');
            void updatePrefs((current) => ({ ...current, qaSaveHistory: next })).catch(() =>
              setError(t('selectionAsk.save.historySaveFailed')),
            );
          }}
        />
      </div>
      {error && (
        <p role="alert" style={{ color: 'var(--ol-red)' }}>
          {error}
        </p>
      )}
      <p style={{ color: 'var(--ol-ink-3)', fontSize: 12, lineHeight: 1.8 }}>
        {t('askAnything.selectionHint')}
      </p>
      {onOpenShortcuts && (
        <button onClick={onOpenShortcuts}>{t('selectionAsk.shortcutSettings')}</button>
      )}
    </div>
  );
}
