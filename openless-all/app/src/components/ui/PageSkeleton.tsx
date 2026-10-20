import { useTranslation } from 'react-i18next';

export function PageSkeleton({ rows = 3 }: { rows?: number }) {
  const { t } = useTranslation();
  return (
    <div
      className="ol-page-skeleton"
      role="status"
      aria-label={t('common.loading')}
      aria-busy="true"
    >
      <div className="ol-skeleton ol-skeleton-heading" />
      {Array.from({ length: rows }, (_, index) => (
        <div className="ol-skeleton ol-skeleton-row" key={index} />
      ))}
    </div>
  );
}
