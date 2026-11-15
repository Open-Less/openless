import { useTranslation } from 'react-i18next';
import { PercentLoader } from './ui/PercentLoader';

/** Startup gate. The percent mark loops until ready and is not a measured ratio. */
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
        {!error && <PercentLoader size={compact ? 28 : 48} />}
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
