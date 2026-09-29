import { useRef, type CSSProperties } from 'react';
import { useExitMount } from '../lib/useExitMount';
import { useOverlayMotion } from '../lib/motion';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { subItemLabelKey } from '../lib/navLabels';
import type { AppTab } from '../state/useAppState';

const STYLE_TABS: Array<{ id: AppTab; icon: string }> = [
  { id: 'style', icon: 'style' },
  { id: 'marketplace', icon: 'sparkle' },
];

interface MobileStyleSheetProps {
  open: boolean;
  currentTab: AppTab;
  onClose: () => void;
  onSelectTab: (tab: AppTab) => void;
}

export function MobileStyleSheet({
  open,
  currentTab,
  onClose,
  onSelectTab,
}: MobileStyleSheetProps) {
  const { t } = useTranslation();
  const { mounted, closing } = useExitMount(open);
  const overlayRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  useOverlayMotion(overlayRef, closing, 'backdrop', mounted);
  useOverlayMotion(cardRef, closing, 'sheet', mounted);
  if (!mounted) return null;

  return (
    <div
      ref={overlayRef}
      onClick={closing ? undefined : onClose}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 60,
        background: 'rgba(15,17,22,0.32)',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'flex-end',
        pointerEvents: closing ? 'none' : undefined,
      }}
    >
      <div
        ref={cardRef}
        role="dialog"
        aria-label={t('nav.group.style')}
        onClick={(e) => e.stopPropagation()}
        style={{
          background: 'var(--ol-surface)',
          borderTopLeftRadius: 16,
          borderTopRightRadius: 16,
          border: '0.5px solid var(--ol-line)',
          padding: '12px 12px calc(12px + env(safe-area-inset-bottom, 0px))',
          boxShadow: '0 -8px 32px -8px rgba(15,17,22,0.18)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '4px 8px 12px',
          }}
        >
          <span style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>
            {t('nav.group.style')}
          </span>
          <button
            type="button"
            onClick={onClose}
            aria-label={t('common.close')}
            style={iconBtnStyle}
          >
            <Icon name="close" size={16} />
          </button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {STYLE_TABS.map((item) => {
            const active = currentTab === item.id;
            return (
              <button
                key={item.id}
                type="button"
                onClick={() => {
                  onSelectTab(item.id);
                  onClose();
                }}
                className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                style={rowBtnStyle}
              >
                <Icon name={item.icon} size={16} />
                <span style={rowLabelStyle}>{t(subItemLabelKey(item.id))}</span>
                <Icon name="chevRight" size={13} />
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const iconBtnStyle: CSSProperties = {
  width: 32,
  height: 32,
  border: 0,
  borderRadius: 999,
  background: 'transparent',
  color: 'var(--ol-ink-3)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  cursor: 'default',
};

const rowBtnStyle: CSSProperties = {
  display: 'flex',
  width: '100%',
  boxSizing: 'border-box',
  minWidth: 0,
  flexWrap: 'nowrap',
  alignItems: 'center',
  gap: 12,
  padding: '12px 14px',
  borderRadius: 10,
  border: 0,
  background: 'transparent',
  fontFamily: 'inherit',
  fontSize: 14,
  cursor: 'default',
  textAlign: 'left',
};

const rowLabelStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
};
