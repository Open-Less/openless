import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  clearExpressionProfile,
  getExpressionProfile,
  type StableExpressionDirection,
} from '../../api/input';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { useHotkeySettings } from '../../data/preferences';
import { Card } from '../../ui/layout/Card';
import { btnGhostStyle } from '../../ui/controls/settingsFieldStyles';
import { ExperimentalSectionTitle } from '../../ui/layout/ExperimentalSectionTitle';
import { SettingRow, Toggle } from '../../ui/controls/SettingRow';

export function ExpressionLearningSection() {
  const { t } = useTranslation();
  const { confirm, notify } = useInteraction();
  const deleteRequest = useRef(false);
  const readGeneration = useRef(0);
  const { prefs, updatePrefs } = useHotkeySettings();
  const [directions, setDirections] = useState<StableExpressionDirection[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    const generation = ++readGeneration.current;
    void getExpressionProfile().then(
      (directions) => {
        if (generation === readGeneration.current) setDirections(directions);
      },
      () => {
        if (generation === readGeneration.current) setError(true);
      },
    );
    return () => {
      readGeneration.current++;
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
          if (deleteRequest.current) return;
          deleteRequest.current = true;
          try {
            await confirm({
              title: t('settings.expressionLearning.clear'),
              description: t('settings.expressionLearning.clearScope'),
              confirmLabel: t('common.clear'),
              tone: 'danger',
              icon: 'trash',
              errorMessage: t('settings.expressionLearning.error'),
              onConfirm: async () => {
                setBusy(true);
                setError(false);
                try {
                  await clearExpressionProfile();
                  readGeneration.current++;
                  setDirections([]);
                  notify({ state: 'deleted', message: t('common.deleted') });
                } finally {
                  setBusy(false);
                }
              },
            });
          } finally {
            deleteRequest.current = false;
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
