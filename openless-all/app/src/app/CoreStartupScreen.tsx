import { useTranslation } from 'react-i18next';

/** Real startup gate: decorative motion ends in 2.4s, but never lies about readiness. */
export function CoreStartupScreen({
  error,
  compact = false,
}: {
  error?: string | null;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="ol-startup" data-compact={compact || undefined}>
      <div className="ol-startup-content" role={error ? 'alert' : 'status'} aria-busy={!error}>
        {!compact && (
          <div className="ol-startup-mark" data-error={!!error} aria-hidden="true">
            <span />
            <span />
            <span />
          </div>
        )}
        <strong>{error ? t('startup.failed') : 'OpenLess'}</strong>
        <p>{t(error ? 'startup.recovery' : 'startup.loading')}</p>
        {error && !compact && (
          <>
            <button
              className="ol-startup-retry"
              type="button"
              onClick={() => window.location.reload()}
            >
              {t('startup.retry')}
            </button>
            <details>
              <summary>{t('startup.details')}</summary>
              <pre>{error}</pre>
            </details>
          </>
        )}
      </div>
    </div>
  );
}
