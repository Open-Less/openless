// Download-only presentation loads on demand. Keep the persistent host-status
// subscription independent of the card rendering and exit lifecycle.
import { AnimatePresence, motion, useIsPresent } from 'framer-motion';
import { useLayoutEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useReducedMotion } from '../../lib/motion';
import { Icon } from '../../components/Icon';
import { useModalActive } from '../../ui/layout/AppViewport';
import type { ProgressItem } from './useDownloadJobs';

export default function DownloadProgressCards({
  visible,
  pending,
  errors,
  onCancel,
}: {
  visible: ProgressItem[];
  pending: string[];
  errors: Record<string, string>;
  onCancel: (item: ProgressItem) => Promise<void>;
}) {
  return (
    <div className="ol-global-downloads">
      <AnimatePresence initial={false}>
        {visible.map((item) => (
          <DownloadProgressCard
            key={item.key}
            item={item}
            pending={pending.includes(item.key)}
            error={errors[item.key]}
            onCancel={onCancel}
          />
        ))}
      </AnimatePresence>
    </div>
  );
}

function DownloadProgressCard({
  item,
  pending,
  error,
  onCancel,
}: {
  item: ProgressItem;
  pending: boolean;
  error?: string;
  onCancel: (item: ProgressItem) => Promise<void>;
}) {
  const { t } = useTranslation();
  const reduced = useReducedMotion();
  const present = useIsPresent();
  const modalActive = useModalActive();
  const blocked = !present || modalActive;
  const cardRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (cardRef.current) cardRef.current.inert = !present;
  }, [present]);
  return (
    <motion.div
      ref={cardRef}
      className="ol-download-card"
      data-download-id={item.id}
      style={{ pointerEvents: blocked ? 'none' : undefined }}
      layout="position"
      initial={{ opacity: 0, transform: reduced ? 'none' : 'translateY(-8px)' }}
      animate={{ opacity: 1, transform: 'translateY(0)' }}
      exit={{ opacity: 0, transform: reduced ? 'none' : 'translateY(-8px)' }}
      transition={{ duration: reduced ? 0 : 0.18 }}
    >
      <div className="ol-download-heading">
        <span className="ol-download-name">{item.name}</span>
        <span role="status" className="ol-download-status">
          {item.percent !== null
            ? `${item.engine === 'experimental' && item.percent < 1 ? '<1' : Math.round(item.percent)}%`
            : t(item.phase ? `experimentalAsr.phases.${item.phase}` : 'localAsr.downloading')}
        </span>
        <button
          type="button"
          disabled={blocked || pending}
          onClick={() => void onCancel(item)}
          aria-label={t('common.cancel')}
          title={t('common.cancel')}
        >
          <Icon name="close" size={12} />
        </button>
      </div>
      <progress aria-label={item.name} max={100} value={item.percent ?? undefined} />
      {error && <p role="alert">{error}</p>}
    </motion.div>
  );
}
