import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import type { referenceBadgeView } from './referenceBadgeView';

export function CapsuleReferenceBadge({ view, reducedMotion }: {
  view: ReturnType<typeof referenceBadgeView>;
  reducedMotion: boolean;
}) {
  const { t } = useTranslation();
  const badge = useRef<HTMLDivElement>(null);
  const count = view?.kind === 'count' ? view.count : undefined;
  const visible = view !== null;

  useEffect(() => {
    if (!visible || reducedMotion || typeof badge.current?.animate !== 'function') return;
    const animation = badge.current.animate(
      [{ transform: 'scale(.94)' }, { transform: 'scale(1.035)', offset: 0.58 }, { transform: 'scale(1)' }],
      { duration: 160, easing: 'ease-out' },
    );
    return () => animation.cancel();
  }, [count, reducedMotion, visible]);

  return (
    <div className="ol-capsule-reference-status" aria-live="polite" aria-atomic="true">
      {view && (
        <div ref={badge} className="ol-capsule-reference-badge" data-warning={view.kind === 'notice'}>
          <Icon name={view.kind === 'notice' ? 'alert' : 'paperclip'} size={13} />
          <span>
            {view.kind === 'notice'
              ? t(`capsule.referenceNotice.${view.notice}`)
              : t('capsule.references', { count: view.count })}
          </span>
        </div>
      )}
    </div>
  );
}
