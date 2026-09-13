import { useEffect, useRef, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Card, PageHeader } from './_atoms';
import { Icon } from '../components/Icon';
import { SavedToast } from '../components/SavedToast';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { formatComboLabel } from '../lib/hotkey';
import type { UserPreferences } from '../lib/types';
import './SelectionAsk.css';

type SaveState = 'idle' | 'saving' | 'saved' | 'failed';

interface SelectionAskProps {
  onOpenShortcuts?: () => void;
}

export function SelectionAsk({ onOpenShortcuts }: SelectionAskProps) {
  const { t } = useTranslation();
  const { prefs, refresh, updatePrefs: savePrefs } = useHotkeySettings();
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [saveMessage, setSaveMessage] = useState('');
  const statusTimer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (statusTimer.current !== null) window.clearTimeout(statusTimer.current);
    },
    [],
  );

  const showSaveStatus = (state: SaveState, message: string, temporary = false) => {
    if (statusTimer.current !== null) {
      window.clearTimeout(statusTimer.current);
      statusTimer.current = null;
    }
    setSaveState(state);
    setSaveMessage(message);
    if (temporary) {
      statusTimer.current = window.setTimeout(() => {
        setSaveState('idle');
        setSaveMessage('');
        statusTimer.current = null;
      }, 1600);
    }
  };

  const persistPrefs = async (
    resolveNext: (current: UserPreferences) => UserPreferences,
    failureMessage: string,
  ) => {
    try {
      await savePrefs(resolveNext);
      showSaveStatus('saved', t('common.saved'), true);
      return true;
    } catch (error) {
      console.error('[selection-ask] failed to save preferences', error);
      showSaveStatus('failed', failureMessage);
      await refresh().catch((refreshError) => {
        console.warn(
          '[selection-ask] failed to refresh preferences after save error',
          refreshError,
        );
      });
      return false;
    }
  };

  if (!prefs) {
    return (
      <>
        <PageHeader title={t('selectionAsk.title')} />
        <Card>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
        </Card>
      </>
    );
  }

  const onSaveHistoryChange = (qaSaveHistory: boolean) => {
    showSaveStatus('saving', t('common.saving'));
    void persistPrefs(
      (current) => ({ ...current, qaSaveHistory }),
      t('selectionAsk.save.historySaveFailed'),
    );
  };

  const currentLabel = prefs.qaHotkey ? formatComboLabel(prefs.qaHotkey) : null;
  const recordHotkeyLabel = formatComboLabel(prefs.dictationHotkey);
  const saving = saveState === 'saving';

  return (
    <div className="ol-selection-ask">
      <PageHeader
        title={t('selectionAsk.title')}
        desc={t('selectionAsk.desc')}
        right={
          onOpenShortcuts && (
            <button type="button" className="ol-selection-ask-settings" onClick={onOpenShortcuts}>
              <Icon name="settings" size={15} />
              {t('selectionAsk.shortcutSettings')}
              <Icon name="chevRight" size={14} />
            </button>
          )
        }
      />
      <SavedToast saveState={saveState} message={saveMessage} />

      <section className="ol-selection-ask-guide" aria-labelledby="selection-ask-guide-title">
        <h2 id="selection-ask-guide-title">{t('selectionAsk.howto.title')}</h2>
        <ol className="ol-selection-ask-steps">
          <li>
            <span className="ol-selection-ask-step-number" aria-hidden="true">
              01
            </span>
            <div>
              <h3>{t('selectionAsk.guide.openTitle')}</h3>
              <p>
                {currentLabel ? (
                  <Trans
                    i18nKey="selectionAsk.guide.openDesc"
                    values={{ hotkey: currentLabel }}
                    components={{ key: <kbd /> }}
                  />
                ) : (
                  t('selectionAsk.guide.unsetDesc')
                )}
              </p>
            </div>
          </li>
          <li>
            <span className="ol-selection-ask-step-number" aria-hidden="true">
              02
            </span>
            <div>
              <h3>{t('selectionAsk.guide.selectTitle')}</h3>
              <p>{t('selectionAsk.howto.step2')}</p>
            </div>
          </li>
          <li>
            <span className="ol-selection-ask-step-number" aria-hidden="true">
              03
            </span>
            <div>
              <h3>{t('selectionAsk.guide.askTitle')}</h3>
              <p>
                <Trans
                  i18nKey="selectionAsk.guide.askDesc"
                  values={{ recordHotkey: recordHotkeyLabel }}
                  components={{ key: <kbd /> }}
                />
              </p>
            </div>
          </li>
        </ol>
        <div className="ol-selection-ask-guide-footer">
          <span>
            <Icon name="refresh" size={14} />
            {t('selectionAsk.guide.followup')}
          </span>
          <span>
            <kbd>Esc</kbd>
            {t('selectionAsk.guide.dismiss')}
          </span>
        </div>
      </section>

      <section className="ol-selection-ask-history" aria-labelledby="selection-ask-history-title">
        <span className="ol-selection-ask-history-icon">
          <Icon name="history" size={20} />
        </span>
        <div className="ol-selection-ask-history-copy">
          <h2 id="selection-ask-history-title">{t('selectionAsk.history.title')}</h2>
          <p id="selection-ask-history-desc">{t('selectionAsk.history.desc')}</p>
        </div>
        <button
          type="button"
          className="ol-selection-ask-switch"
          role="switch"
          aria-checked={prefs.qaSaveHistory}
          aria-labelledby="selection-ask-history-title"
          aria-describedby="selection-ask-history-desc"
          onClick={() => onSaveHistoryChange(!prefs.qaSaveHistory)}
          disabled={saving}
        >
          <span />
        </button>
      </section>
    </div>
  );
}
