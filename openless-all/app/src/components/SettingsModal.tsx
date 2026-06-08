// SettingsModal.tsx — 居中弹窗，左侧单层侧栏。
//
// 重构（2026-05）：原本是「外层弹窗侧栏 + 设置页内层侧栏」双层嵌套，用户点
// 「设置」还要再面对第二个侧栏。现在拍平成单层 —— 通用 / 服务 / 隐私 / 高级 /
// 个性化 / 关于 六个 tab。每个 tab 的内容见 pages/settings/。
//
// 设计原则：每个可见控件都必须可用。没有后端支撑的占位（账号 / 主题切换 等）
// 不在此弹窗出现。

import { useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { SavedToast } from './SavedToast';
import { useSavedToastListener } from '../lib/savedEvent';
import type { OS } from './WindowChrome';
import { GeneralTab, ServicesTab, PrivacyTab, AdvancedTab } from '../pages/settings/tabs';
import { AboutSection } from '../pages/settings/AboutSection';
import { useMobileLayout } from '../lib/useMobileLayout';

// 稳定 tab ID（与 i18n key `modal.sections.*` 一致）。
export type SettingsSectionId =
  | 'general'
  | 'services'
  | 'privacy'
  | 'advanced'
  | 'about';

interface SettingsModalProps {
  os: OS;
  onClose: () => void;
  initialSettingsSection?: SettingsSectionId;
}

interface ModalNavItem {
  id: string;
  icon: string;
}

const TAB_ITEMS: ModalNavItem[] = [
  { id: 'general', icon: 'settings' },
  { id: 'services', icon: 'cloud' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'bolt' },
  { id: 'about', icon: 'info' },
];

export function SettingsModal({ os: _os, onClose, initialSettingsSection }: SettingsModalProps) {
  const { t } = useTranslation();
  const [section, setSection] = useState<SettingsSectionId>(initialSettingsSection ?? 'general');
  const savedToast = useSavedToastListener();
  const mobile = useMobileLayout();

  // 与 sidebar nav 一致的滑动指示器：仅 tab 组有 pill；外链组永远不画 pill。
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [pillRect, setPillRect] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    if (mobile) {
      setPillRect(null);
      return;
    }
    const idx = TAB_ITEMS.findIndex(it => it.id === section);
    const el = tabRefs.current[idx];
    if (!el) return;
    setPillRect({ top: el.offsetTop, height: el.offsetHeight });
  }, [section, mobile]);

  return (
    <div
      onClick={onClose}
      style={{
        position: 'absolute', inset: 0,
        background: 'var(--ol-overlay-bg)',
        backdropFilter: 'blur(8px) saturate(140%)',
        WebkitBackdropFilter: 'blur(8px) saturate(140%)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: mobile ? 0 : 28,
        zIndex: 50,
        animation: 'ol-modal-backdrop-in 0.18s var(--ol-motion-soft)',
      }}>

      <div
        className="ol-aura-settings"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%',
          maxWidth: 920,
          height: '100%',
          maxHeight: mobile ? 'none' : 620,
          display: 'flex',
          flexDirection: mobile ? 'column' : 'row',
          overflow: 'hidden',
          animation: 'ol-modal-card-in 0.24s var(--ol-motion-spring)',
          position: 'relative',
        }}>

        {/* ─── 单层侧栏 ────────────────────────────────────────────── */}
        <aside
          className="ol-aura-settings-rail"
          style={{
            width: mobile ? '100%' : 214,
            flexShrink: 0,
            display: 'flex',
            flexDirection: mobile ? 'row' : 'column',
          }}>

          {/* tab 组 */}
          <div
            className="ol-thinscroll"
            style={{
              position: 'relative',
              display: 'flex',
              flexDirection: mobile ? 'row' : 'column',
              gap: 1,
              minWidth: 0,
              flex: mobile ? '1 1 auto' : undefined,
              overflowX: mobile ? 'auto' : undefined,
            }}
          >
            {pillRect && (
              <div
                className="ol-aura-settings-pill"
                aria-hidden
                style={{
                  position: 'absolute',
                  left: 0,
                  right: 0,
                  top: pillRect.top,
                  height: pillRect.height,
                  transition: 'top 0.36s var(--ol-motion-spring), height 0.36s var(--ol-motion-spring)',
                  pointerEvents: 'none',
                  zIndex: 0,
                }}
              />
            )}
            {TAB_ITEMS.map((it, idx) => {
              const active = section === it.id;
              return (
                <button
                  key={it.id}
                  ref={el => { tabRefs.current[idx] = el; }}
                  onClick={() => setSection(it.id as SettingsSectionId)}
                  className={active ? 'ol-nav-btn ol-nav-btn-active ol-aura-settings-nav-btn' : 'ol-nav-btn ol-aura-settings-nav-btn'}
                  style={{ ...navBtnStyle, flexShrink: 0 }}>
                  <Icon name={it.icon} size={14} />
                  <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                </button>
              );
            })}
          </div>
        </aside>

        {/* ─── 内容区 ──────────────────────────────────────────────
            父容器 overflow:hidden + 列向 flex；关闭按钮、section 标题固定在头部，
            只有最里层的 scroll wrapper 真正滚动。 */}
        <div
          className="ol-aura-settings-content"
          style={{
            flex: 1,
            minWidth: 0,
            overflow: 'hidden',
            position: 'relative',
            display: 'flex',
            flexDirection: 'column',
          }}
        >
          {/* "已保存" toast：right:54 避开 28×28 关闭按钮 + 12px gap。 */}
          <SavedToast
            saveState={savedToast.state}
            message={savedToast.message}
            slideFrom="top"
            offsetStyle={{ position: 'absolute', top: 16, right: 54 }}
          />
          <button
            className="ol-aura-settings-close"
            onClick={onClose}
            style={{
              position: 'absolute', top: 14, right: 14, zIndex: 2,
              width: 28, height: 28, border: 0, borderRadius: 999,
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              cursor: 'default',
            }}
            title={t('common.close')}>
            <Icon name="close" size={14} />
          </button>

          <h2 className="ol-aura-settings-title" style={{ margin: 0, flexShrink: 0 }}>
            {t(`modal.sections.${section}`)}
          </h2>

          <div
            className="ol-thinscroll"
            style={{
              flex: 1,
              minHeight: 0,
              overflow: 'auto',
              padding: mobile
                ? '8px 14px calc(18px + env(safe-area-inset-bottom, 0px))'
                : '10px 28px 28px',
            }}>
            {/* key=section 让切 tab 时整块重挂载，ol-tab-fade 轻微淡入。 */}
            <div
              key={section}
              style={{ display: 'flex', flexDirection: 'column', gap: 12, animation: 'ol-tab-fade 0.2s var(--ol-motion-soft)' }}>
              {section === 'general' && <GeneralTab />}
              {section === 'services' && <ServicesTab />}
              {section === 'privacy' && <PrivacyTab />}
              {section === 'advanced' && <AdvancedTab />}
              {section === 'about' && <AboutSection />}
            </div>
          </div>
        </div>
      </div>
      <style>{`
        .ol-aura-settings {
          background: var(--ol-panel-bg);
          border-radius: ${mobile ? '0' : 'var(--ol-shell-radius)'};
          border: 1px solid var(--ol-panel-border);
          box-shadow: var(--ol-panel-shadow);
        }
        .ol-aura-settings-rail {
          padding: ${mobile ? 'calc(10px + env(safe-area-inset-top, 0px)) 10px 8px' : '20px 14px'};
          gap: ${mobile ? '8px' : '16px'};
          background: var(--ol-settings-rail-bg);
          border-right: ${mobile ? '0' : '1px solid var(--ol-settings-rail-border)'};
          border-bottom: ${mobile ? '1px solid var(--ol-settings-rail-border)' : '0'};
        }
        .ol-aura-settings-pill {
          background: var(--ol-sidebar-pill-bg);
          border-radius: 12px;
          border: 1px solid var(--ol-sidebar-pill-border);
          box-shadow: none;
        }
        .ol-aura-settings-nav-btn {
          padding: ${mobile ? '8px 11px' : '7px 10px'};
          border-radius: 12px;
          border: 0;
          background: transparent;
          font-family: inherit;
          font-size: 13px;
          cursor: default;
          text-align: left;
          position: relative;
          z-index: 1;
          transition: color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick);
        }
        .ol-aura-settings-nav-btn.ol-nav-btn-active {
          background: ${mobile ? 'var(--ol-sidebar-pill-bg)' : 'transparent'};
          border: ${mobile ? '1px solid var(--ol-sidebar-pill-border)' : '0'};
        }
        .ol-aura-settings-links {
          display: flex;
          flex-direction: ${mobile ? 'row' : 'column'};
          gap: 1px;
          padding-top: ${mobile ? '0' : '10px'};
          padding-left: ${mobile ? '8px' : '0'};
          border-top: ${mobile ? '0' : '1px solid var(--ol-settings-links-border)'};
          border-left: ${mobile ? '1px solid var(--ol-settings-links-border)' : '0'};
          overflow-x: ${mobile ? 'auto' : 'visible'};
        }
        .ol-aura-settings-content {
          background: var(--ol-settings-content-bg);
        }
        .ol-aura-settings-close {
          background: var(--ol-settings-close-bg);
          color: var(--ol-ink-3);
          transition: background 0.16s var(--ol-motion-quick);
        }
        .ol-aura-settings-close:hover {
          background: var(--ol-settings-close-hover-bg);
        }
        .ol-aura-settings-title {
          padding: ${mobile ? '16px 48px 8px 16px' : '24px 28px 10px'};
          font-size: ${mobile ? '20px' : '22px'};
          font-weight: 600;
          letter-spacing: -0.02em;
          font-family: var(--ol-font-display);
          color: var(--ol-ink);
        }
      `}</style>
    </div>
  );
}

const navBtnStyle = {
  display: 'flex', alignItems: 'center', gap: 10,
};
