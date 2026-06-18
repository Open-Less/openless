// MarketplaceModal.tsx — 风格市场弹窗。
// 跟 SettingsModal 同款 backdrop + 居中卡片。内容直接复用 <Marketplace />。
// 入口在 Style 页面「风格包」标题右侧，由 Style.tsx 控制 open/close。
// 顶部 pill 显示当前「登录身份」（dev 模式 = marketplaceDevLogin），未填时引导跳 Settings。

import { useEffect, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { Marketplace } from '../pages/Marketplace';
import { useHotkeySettings } from '../state/HotkeySettingsContext';

interface MarketplaceModalProps {
  onClose: () => void;
}

const modalChipDarkStyle: CSSProperties = {
  background: 'var(--ol-pill-selected-bg)',
  color: 'var(--ol-pill-selected-ink)',
  border: '0.5px solid var(--ol-pill-selected-border)',
  boxShadow: 'var(--ol-shadow-sm)',
};

const modalCloseBtnStyle: CSSProperties = {
  ...modalChipDarkStyle,
  position: 'absolute',
  top: 16,
  right: 16,
  zIndex: 3,
  width: 32,
  height: 32,
  borderRadius: 8,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  cursor: 'default',
  transition: 'opacity 0.16s var(--ol-motion-quick)',
};

export function MarketplaceModal({ onClose }: MarketplaceModalProps) {
  const { t } = useTranslation();
  const { prefs } = useHotkeySettings();
  const login = (prefs?.marketplaceDevLogin ?? '').trim();
  const loggedIn = login.length > 0;
  // Esc 关闭
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  return (
    <div
      onClick={onClose}
      style={{
        position: 'absolute', inset: 0,
        background: 'rgba(15,17,22,0.32)',
        backdropFilter: 'blur(8px) saturate(140%)',
        WebkitBackdropFilter: 'blur(8px) saturate(140%)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 28,
        zIndex: 50,
        animation: 'ol-modal-backdrop-in 0.18s var(--ol-motion-soft)',
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%', maxWidth: 1080, height: '100%', maxHeight: 720,
          background: 'var(--ol-surface)',
          borderRadius: 14,
          border: '0.5px solid rgba(0,0,0,.08)',
          boxShadow: '0 30px 80px -20px rgba(15,17,22,.35), 0 0 0 0.5px rgba(0,0,0,.06)',
          overflow: 'hidden',
          position: 'relative',
          display: 'flex', flexDirection: 'column',
          animation: 'ol-modal-card-in 0.24s var(--ol-motion-spring)',
        }}
      >
        <div
          style={{
            position: 'absolute', top: 16, left: 24, zIndex: 3,
            display: 'inline-flex', alignItems: 'center', gap: 8,
          }}
        >
          <button
            type="button"
            tabIndex={-1}
            title={loggedIn ? t('marketplace.modal.loggedIn') : t('marketplace.modal.notLoggedIn')}
            style={{
              display: 'inline-flex', alignItems: 'center', gap: 6,
              padding: '4px 10px', borderRadius: 999,
              fontSize: 11.5, fontWeight: 500,
              cursor: 'default',
              ...(loggedIn
                ? modalChipDarkStyle
                : {
                    border: '0.5px solid rgba(239,68,68,0.32)',
                    background: 'rgba(239,68,68,0.08)',
                    color: 'var(--ol-red, #ef4444)',
                    boxShadow: '0 1px 2px rgba(15,17,22,0.05)',
                  }),
            }}
          >
            <Icon name="user" size={11} />
            <span>{loggedIn ? `@${login}` : t('marketplace.modal.notLoggedInLabel')}</span>
          </button>
        </div>

        <button
          onClick={onClose}
          style={modalCloseBtnStyle}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLButtonElement).style.opacity = '0.85';
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLButtonElement).style.opacity = '1';
          }}
          aria-label={t('common.close')}
          title={t('common.close')}
        >
          <Icon name="close" size={14} />
        </button>
        {/* paddingTop 64 给 X 按钮留位置 —— PageHeader 的 right 槽（刷新/上传）会下沉到 X 下方 */}
        <div style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: '64px 32px 32px' }}>
          <Marketplace />
        </div>
      </div>
    </div>
  );
}
