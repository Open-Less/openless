import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { formatComboLabel } from '../../api/hotkeys';
import type { QaShortcutBehavior } from '../../lib/types';
import { setQaHotkey } from '../../api/qa';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { useHotkeySettings } from '../../data/preferences';
import { PageHeader } from '../../ui/layout/PageHeader';
import { Toggle } from '../../ui/controls/SettingRow';
import './SelectionAsk.css';

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
      <label className="ol-ask-settings-row">
        <div>
          <strong>{t('askAnything.shortcutBehavior.title')}</strong>
          <p>{t('askAnything.shortcutBehavior.desc')}</p>
        </div>
        <select
          value={prefs.qaShortcutBehavior ?? 'record'}
          onChange={(event) => {
            const qaShortcutBehavior = event.currentTarget.value as QaShortcutBehavior;
            setError('');
            void updatePrefs((current) => ({ ...current, qaShortcutBehavior })).catch(() =>
              setError(t('askAnything.shortcutBehavior.failed')),
            );
          }}
        >
          <option value="record">{t('askAnything.shortcutBehavior.record')}</option>
          <option value="open">{t('askAnything.shortcutBehavior.open')}</option>
          <option value="draft">{t('askAnything.shortcutBehavior.draft')}</option>
        </select>
      </label>
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
