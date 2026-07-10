// SettingsModal.tsx — 居中弹窗，左侧单层侧栏。
//
// 重构（2026-05）：原本是「外层弹窗侧栏 + 设置页内层侧栏」双层嵌套，用户点
// 「设置」还要再面对第二个侧栏。现在拍平成单层 —— 通用 / 服务 / 隐私 / 高级 /
// 关于 五个 tab + 帮助外链组。每个 tab 的内容见 pages/settings/tabs.tsx，
// 分组原则：按「用户带着什么问题来」归类 —— 怎么录（通用）/ 识别润色由谁提供
// （服务，含本地模型）/ 数据去哪了（隐私）/ 实验功能（高级）/ 版本与更新（关于）。
//
// 设计原则：每个可见控件都必须可用。

import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { SavedToast } from './SavedToast';
import { useSavedToastListener } from '../lib/savedEvent';
import { openExternal } from '../lib/ipc';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { OS } from './WindowChrome';
import { AboutTab, GeneralTab, ServicesTab, PrivacyTab, AdvancedTab } from '../pages/settings/tabs';
import { chipSelectedStyle } from '../pages/settings/shared';

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
  external?: boolean;
  href?: string;
}

const HELP_URL = 'https://github.com/appergb/openless#readme';
const RELEASE_NOTES_URL = 'https://github.com/appergb/openless/releases';

// 第一组：可选中的 tab；第二组：外部链接（永远不 active）。
const TAB_ITEMS: ModalNavItem[] = [
  { id: 'general', icon: 'settings' },
  { id: 'services', icon: 'cloud' },
  { id: 'privacy', icon: 'shield' },
  { id: 'advanced', icon: 'bolt' },
  { id: 'about', icon: 'info' },
];
const LINK_ITEMS: ModalNavItem[] = [
  { id: 'helpCenter', icon: 'help', external: true, href: HELP_URL },
  { id: 'releaseNotes', icon: 'doc', external: true, href: RELEASE_NOTES_URL },
];

export function SettingsModal({ os: _os, onClose, initialSettingsSection }: SettingsModalProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [section, setSection] = useState<SettingsSectionId>(initialSettingsSection ?? 'general');
  const savedToast = useSavedToastListener();

  // 与 sidebar nav 一致的滑动指示器：仅 tab 组有 pill；外链组永远不画 pill（desktop）。
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
      onClick={mobile ? undefined : onClose}
      style={{
        position: mobile ? 'fixed' : 'absolute',
        inset: 0,
        // 白/实底哲学：遮罩只做半透明压暗（overlay token），不再叠 backdrop-filter 玻璃模糊。
        background: mobile ? 'var(--ol-surface)' : 'var(--ol-overlay-bg)',
        display: 'flex',
        alignItems: mobile ? 'stretch' : 'center',
        justifyContent: mobile ? 'stretch' : 'center',
        padding: mobile ? 0 : 28,
        zIndex: 50,
        animation: mobile ? undefined : 'ol-modal-backdrop-in 0.18s var(--ol-motion-soft)',
      }}>

      <div
        className="ol-settings-surface"
        data-ol-mobile={mobile ? 'true' : undefined}
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%',
          maxWidth: mobile ? undefined : 880,
          height: '100%',
          maxHeight: mobile ? undefined : 600,
          background: 'var(--ol-settings-content-bg)',
          borderRadius: mobile ? 0 : 14,
          border: mobile ? 'none' : '0.5px solid var(--ol-line)',
          boxShadow: mobile ? 'none' : 'var(--ol-shadow-xl)',
          display: 'flex',
          flexDirection: mobile ? 'column' : 'row',
          overflow: 'hidden',
          animation: mobile ? undefined : 'ol-modal-card-in 0.24s var(--ol-motion-spring)',
          position: 'relative',
        }}>

        {mobile ? (
          <div style={{
            flexShrink: 0,
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: 'calc(10px + env(safe-area-inset-top, 0px)) 12px 10px',
            borderBottom: '0.5px solid var(--ol-line-soft)',
          }}>
            <button
              type="button"
              onClick={onClose}
              aria-label={t('common.close')}
              style={mobileHeaderBtnStyle}
            >
              <Icon name="close" size={16} />
            </button>
            <div
              className="ol-thinscroll"
              style={{ flex: 1, minWidth: 0, display: 'flex', gap: 6, overflowX: 'auto', paddingBottom: 2 }}
            >
              {TAB_ITEMS.map(it => {
                const active = section === it.id;
                return (
                  <button
                    key={it.id}
                    type="button"
                    onClick={() => setSection(it.id as SettingsSectionId)}
                    className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                    style={mobileTabChipStyle(active)}
                  >
                    {t(`modal.sections.${it.id}`)}
                  </button>
                );
              })}
            </div>
          </div>
        ) : (
        <aside
          style={{
            width: 200, flexShrink: 0,
            background: 'var(--ol-settings-rail-bg)',
            borderRight: '0.5px solid var(--ol-line-soft)',
            padding: '18px 12px',
            display: 'flex', flexDirection: 'column', gap: 14,
          }}>

          {/* tab 组 */}
          <div style={{ position: 'relative', display: 'flex', flexDirection: 'column', gap: 1 }}>
            {pillRect && (
              <div
                aria-hidden
                style={{
                  position: 'absolute',
                  left: 0,
                  right: 0,
                  top: pillRect.top,
                  height: pillRect.height,
                  background: 'var(--ol-segmented-active-bg)',
                  borderRadius: 8,
                  boxShadow: 'var(--ol-segmented-active-shadow)',
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
                  className={active ? 'ol-nav-btn ol-nav-btn-active' : 'ol-nav-btn'}
                  style={navBtnStyle}>
                  <Icon name={it.icon} size={14} />
                  <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                </button>
              );
            })}
          </div>

          {/* 外链组 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 1, paddingTop: 8, borderTop: '0.5px solid var(--ol-line-soft)' }}>
            {LINK_ITEMS.map(it => (
              <button
                key={it.id}
                onClick={() => { if (it.href) void openExternal(it.href); }}
                className="ol-nav-btn"
                style={navBtnStyle}>
                <Icon name={it.icon} size={14} />
                <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                <Icon name="external" size={11} />
              </button>
            ))}
          </div>
        </aside>
        )}

        {/* ─── 内容区 ────────────────────────────────────────────── */}
        <div style={{ flex: 1, minWidth: 0, overflow: 'hidden', position: 'relative', display: 'flex', flexDirection: 'column' }}>
          <SavedToast
            saveState={savedToast.state}
            message={savedToast.message}
            slideFrom="top"
            offsetStyle={{ position: 'absolute', top: mobile ? 12 : 16, right: mobile ? 14 : 54 }}
          />
          {!mobile && (
          <button
            onClick={onClose}
            style={{
              position: 'absolute', top: 14, right: 14, zIndex: 2,
              width: 28, height: 28, border: 0, borderRadius: 999,
              background: 'transparent', color: 'var(--ol-ink-3)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              cursor: 'default',
              transition: 'background 0.16s var(--ol-motion-quick)',
            }}
            // hover 底色适配白/实底内容区：旧 token 是白玻璃 rgba（白底上不可见），改用 surface-2。
            onMouseEnter={e => (e.currentTarget.style.background = 'var(--ol-surface-2)')}
            onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
            title={t('common.close')}>
            <Icon name="close" size={14} />
          </button>
          )}

          {!mobile && (
          <h2 style={{ margin: 0, padding: '22px 28px 8px', fontSize: 22, fontWeight: 600, letterSpacing: '-0.02em', flexShrink: 0 }}>
            {t(`modal.sections.${section}`)}
          </h2>
          )}

          <div
            className="ol-thinscroll ol-scroll-fade"
            style={{
              flex: 1,
              minHeight: 0,
              overflow: 'auto',
              padding: mobile ? '12px 16px calc(16px + env(safe-area-inset-bottom, 0px))' : '10px 28px 28px',
            }}>
            {/* key=section 让切 tab 时整块重挂载，ol-tab-fade 轻微淡入。 */}
            <div
              key={section}
              style={{ display: 'flex', flexDirection: 'column', gap: 12, animation: 'ol-tab-fade 0.2s var(--ol-motion-soft)' }}>
              {section === 'general' && <GeneralTab />}
              {section === 'services' && <ServicesTab />}
              {section === 'privacy' && <PrivacyTab />}
              {section === 'advanced' && <AdvancedTab />}
              {section === 'about' && <AboutTab />}
            </div>
            {mobile && (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 20, paddingTop: 16, borderTop: '0.5px solid var(--ol-line-soft)' }}>
                {LINK_ITEMS.map(it => (
                  <button
                    key={it.id}
                    type="button"
                    onClick={() => { if (it.href) void openExternal(it.href); }}
                    className="ol-nav-btn"
                    style={navBtnStyle}
                  >
                    <Icon name={it.icon} size={14} />
                    <span style={{ flex: 1 }}>{t(`modal.sections.${it.id}`)}</span>
                    <Icon name="external" size={11} />
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

const mobileHeaderBtnStyle: CSSProperties = {
  width: 36,
  height: 36,
  flexShrink: 0,
  border: 0,
  borderRadius: 10,
  background: 'transparent',
  color: 'var(--ol-ink-3)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  cursor: 'default',
};

function mobileTabChipStyle(active: boolean): CSSProperties {
  return {
    flexShrink: 0,
    padding: '6px 12px',
    borderRadius: 999,
    fontFamily: 'inherit',
    fontSize: 12,
    fontWeight: active ? 600 : 500,
    cursor: 'default',
    ...chipSelectedStyle(active),
  };
}

const navBtnStyle = {
  display: 'flex', alignItems: 'center', gap: 10,
  padding: '7px 10px',
  borderRadius: 8, border: 0,
  background: 'transparent',
  fontFamily: 'inherit', fontSize: 13,
  cursor: 'default', textAlign: 'left' as const,
  position: 'relative' as const,
  zIndex: 1,
  transition: 'color 0.16s var(--ol-motion-quick), background 0.16s var(--ol-motion-quick)',
};
