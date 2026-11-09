import { useTranslation } from 'react-i18next';
import { startupReport } from '../../lib/startupMetrics';

export function StartupDiagnostics() {
  const { t } = useTranslation();
  return (
    <div className="ol-startup-diagnostics">
      <p className="ol-section-note">{t('uiRefinement.startupHint')}</p>
      <pre>{JSON.stringify(startupReport(), null, 2)}</pre>
    </div>
  );
}
