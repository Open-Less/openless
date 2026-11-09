import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  clearExpressionProfile,
  getExpressionProfile,
  type StableExpressionDirection,
} from '../../lib/ipc/expression';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import { btnGhostStyle, ExperimentalSectionTitle, SettingRow, Toggle } from './shared';

export function ExpressionLearningSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [directions, setDirections] = useState<StableExpressionDirection[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    let alive = true;
    void getExpressionProfile().then(
      (directions) => {
        if (alive) setDirections(directions);
      },
      () => {
        if (alive) setError(true);
      },
    );
    return () => {
      alive = false;
    };
  }, []);
  if (!prefs) return null;
  return (
    <Card>
      <ExperimentalSectionTitle badge={t('common.experimental')}>
        {t('settings.expressionLearning.title')}
      </ExperimentalSectionTitle>
      <SettingRow
        label={t('settings.expressionLearning.enabled')}
        desc={t('settings.expressionLearning.description')}
      >
        <Toggle
          label={t('settings.expressionLearning.enabled')}
          on={prefs.expressionLearningEnabled}
          disabled={busy || (directions === null && !prefs.expressionLearningEnabled)}
          onToggle={async (next) => {
            setBusy(true);
            setError(false);
            try {
              await updatePrefs((current) => ({ ...current, expressionLearningEnabled: next }));
              if (directions === null) setError(true);
            } catch {
              setError(true);
            } finally {
              setBusy(false);
            }
          }}
        />
      </SettingRow>
      <p style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {directions === null
          ? error
            ? ''
            : t('common.loading')
          : t('settings.expressionLearning.status', { count: directions.length })}
      </p>
      {directions && directions.length > 0 && (
        <ul style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
          {directions.map((item) => (
            <li key={item.feature}>
              {t(
                `settings.expressionLearning.${item.feature}${item.direction > 0 ? 'More' : 'Less'}`,
              )}
            </li>
          ))}
        </ul>
      )}
      <button
        type="button"
        className="ol-expression-clear"
        style={btnGhostStyle}
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          setError(false);
          try {
            await clearExpressionProfile();
            setDirections([]);
          } catch {
            setError(true);
          } finally {
            setBusy(false);
          }
        }}
      >
        {t('settings.expressionLearning.clear')}
      </button>
      <p style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6 }}>
        {t('settings.expressionLearning.clearHint')}
      </p>
      {error && <p role="alert">{t('settings.expressionLearning.error')}</p>}
    </Card>
  );
}
