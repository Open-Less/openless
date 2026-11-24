import { useTranslation } from 'react-i18next';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { useInAppDictation } from './useInAppDictation';

export function HarmonyDictation({ onComplete }: { onComplete: () => void }) {
  const { t } = useTranslation();
  const { state, result, copied, record, cancel, copy } = useInAppDictation(onComplete);
  const waiting = ['starting', 'processing', 'cancelling'].includes(state.phase);
  return (
    <Card padding={16} style={{ flexShrink: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
        <strong style={{ flex: 1, fontSize: 14 }}>{t('overview.inAppDictation.title')}</strong>
        <span role="status" style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
          {t(`overview.inAppDictation.${waiting ? 'processing' : state.phase}`)}
        </span>
        <Btn size="sm" onClick={() => void record()} disabled={waiting}>
          {t(`overview.inAppDictation.${state.phase === 'recording' ? 'stop' : 'start'}`)}
        </Btn>
        {state.phase !== 'idle' && (
          <Btn
            size="sm"
            variant="soft"
            disabled={state.phase === 'cancelling'}
            onClick={() => void cancel()}
          >
            {t('common.cancel')}
          </Btn>
        )}
      </div>
      {state.phase === 'recording' && (
        <progress
          aria-label={t('overview.inAppDictation.recording')}
          value={state.level}
          max={1}
          style={{ width: '100%', marginTop: 12 }}
        />
      )}
      {state.error && (
        <div role="alert" style={{ color: 'var(--ol-danger)', fontSize: 13, marginTop: 10 }}>
          {state.error}
        </div>
      )}
      {result && (
        <div style={{ marginTop: 12 }}>
          <div
            style={{
              whiteSpace: 'pre-wrap',
              overflowWrap: 'anywhere',
              maxHeight: 160,
              overflowY: 'auto',
              fontSize: 14,
            }}
          >
            {result}
          </div>
          <Btn size="sm" variant="soft" onClick={() => void copy()}>
            {t(copied ? 'common.copied' : 'common.copy')}
          </Btn>
        </div>
      )}
    </Card>
  );
}
