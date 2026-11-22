import './downloads.css';

// Host jobs outlive settings. Experimental downloads use the same authoritative
// snapshot and cancellation route as their switches, not the retired Qwen API.
import { lazy, Suspense, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useDownloadJobs } from './useDownloadJobs';
export type { ProgressItem } from './useDownloadJobs';
const DownloadProgressCards = lazy(() => import('./DownloadProgressCards'));

export function GlobalDownloadProgress() {
  const { t } = useTranslation();
  const { visible, pending, errors, handleCancel } = useDownloadJobs();
  const activated = useRef(false);
  if (visible.length) activated.current = true;
  if (!activated.current) return null;
  return (
    <Suspense
      fallback={
        visible.length ? (
          <div className="ol-global-downloads" role="status">
            {t('localAsr.downloading')}
          </div>
        ) : null
      }
    >
      <DownloadProgressCards
        visible={visible}
        errors={errors}
        pending={pending}
        onCancel={handleCancel}
      />
    </Suspense>
  );
}
