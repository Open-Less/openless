import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';

/** All windows keep the same contract gate; only its presentation varies by available space. */
export function CoreStartupScreen({
  error,
  compact = false,
}: {
  error?: string | null;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div
      style={{
        minHeight: '100%',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: compact ? 8 : 24,
        background: 'var(--ol-surface)',
        color: 'var(--ol-ink)',
      }}
    >
      <div style={{ width: '100%', maxWidth: 420 }}>
        <div
          role={error ? 'alert' : 'status'}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            fontSize: compact ? 12 : 18,
            fontWeight: 600,
          }}
        >
          {!compact && <Icon name={error ? 'info' : 'refresh'} size={22} />}
          {t(error ? 'startup.failed' : 'startup.loading')}
        </div>
        {!compact && (
          <>
            <p
              style={{
                fontSize: 13,
                lineHeight: 1.65,
                color: 'var(--ol-ink-3)',
                margin: '12px 0 18px',
              }}
            >
              {t(error ? 'startup.recovery' : 'startup.loadingDesc')}
            </p>
            {error && (
              <>
                <button
                  type="button"
                  onClick={() => window.location.reload()}
                  style={{
                    background: 'var(--ol-primary-solid-bg)',
                    color: 'var(--ol-primary-solid-ink)',
                    border: 0,
                    borderRadius: 8,
                    padding: '10px 16px',
                    font: 'inherit',
                    fontSize: 13,
                    cursor: 'pointer',
                  }}
                >
                  {t('startup.retry')}
                </button>
                <details style={{ marginTop: 20, fontSize: 12, color: 'var(--ol-ink-3)' }}>
                  <summary style={{ cursor: 'pointer' }}>{t('startup.details')}</summary>
                  <pre
                    style={{
                      whiteSpace: 'pre-wrap',
                      overflowWrap: 'anywhere',
                      fontSize: 11,
                      lineHeight: 1.6,
                      marginTop: 10,
                    }}
                  >
                    {error}
                  </pre>
                </details>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
