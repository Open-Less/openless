// SettingsModal.tsx — centered sheet with sub-nav on the left.
// Ported verbatim from design_handoff_openless/variants.jsx::SettingsModal
// (plus its AccountSection / PersonalizeSection / AboutMini siblings).

import { useEffect, useState, type CSSProperties } from 'react';
import { Icon } from './Icon';
import { APP_VERSION_LABEL } from '../lib/appVersion';
import { Settings as SettingsContent, type SettingsSectionId } from '../pages/Settings';
import { Row } from './ui/Row';
import { SegSimple } from './ui/SegSimple';
import { SwitchLite } from './ui/SwitchLite';
import { SelectLite } from './ui/SelectLite';
import type { OS } from './WindowChrome';

interface SettingsModalProps {
  os: OS;
  onClose: () => void;
  initialSettingsSection?: SettingsSectionId;
}

type ModalSectionId = '账户' | '设置' | '个性化' | '关于';

interface ModalNavItem {
  id: string;
  icon: string;
  external?: boolean;
}

interface ModalGroup {
  items: ModalNavItem[];
}

export function SettingsModal({ os: _os, onClose, initialSettingsSection }: SettingsModalProps) {
  const [section, setSection] = useState<ModalSectionId>('设置');
  const groups: ModalGroup[] = [
    { items: [{ id: '账户', icon: 'user' }, { id: '设置', icon: 'settings' }, { id: '个性化', icon: 'sparkle' }, { id: '关于', icon: 'info' }] },
    { items: [{ id: '帮助中心', icon: 'help', external: true }, { id: '版本说明', icon: 'doc', external: true }] },
  ];

  return (
    <div
      onClick={onClose}
      style={{
        position: 'absolute', inset: 0,
        background: 'rgba(15,17,22,0.32)',
        backdropFilter: 'blur(2px)',
        WebkitBackdropFilter: 'blur(2px)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        padding: 28,
        zIndex: 50,
        animation: 'ol-modal-fade .18s ease-out',
      }}>

      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%', maxWidth: 880, height: '100%', maxHeight: 600,
          background: 'var(--ol-surface)',
          borderRadius: 14,
          border: '0.5px solid rgba(0,0,0,.08)',
          boxShadow: '0 30px 80px -20px rgba(15,17,22,.35), 0 0 0 0.5px rgba(0,0,0,.06)',
          display: 'flex', overflow: 'hidden',
          animation: 'ol-modal-pop .22s cubic-bezier(.2,.9,.3,1.1)',
          position: 'relative',
        }}>

        {/* sub-sidebar */}
        <aside
          style={{
            width: 200, flexShrink: 0,
            background: 'rgba(247,247,250,0.7)',
            borderRight: '0.5px solid var(--ol-line-soft)',
            padding: '18px 12px',
            display: 'flex', flexDirection: 'column', gap: 14,
          }}>

          {groups.map((g, gi) => (
            <div key={gi} style={{ display: 'flex', flexDirection: 'column', gap: 1, paddingTop: gi === 1 ? 8 : 0, borderTop: gi === 1 ? '0.5px solid var(--ol-line-soft)' : 'none' }}>
              {g.items.map((it) => {
                const active = section === it.id && !it.external;
                return (
                  <button
                    key={it.id}
                    onClick={() => !it.external && setSection(it.id as ModalSectionId)}
                    style={{
                      display: 'flex', alignItems: 'center', gap: 10,
                      padding: '7px 10px',
                      borderRadius: 8, border: 0,
                      background: active ? '#fff' : 'transparent',
                      color: active ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                      fontFamily: 'inherit', fontSize: 13, fontWeight: active ? 600 : 500,
                      boxShadow: active ? '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)' : 'none',
                      cursor: 'default', textAlign: 'left',
                    }}>

                    <Icon name={it.icon} size={14} />
                    <span style={{ flex: 1 }}>{it.id}</span>
                    {it.external && <Icon name="external" size={11} />}
                  </button>
                );
              })}
            </div>
          ))}
        </aside>

        {/* content */}
        <div style={{ flex: 1, minWidth: 0, overflow: 'auto', padding: '22px 28px 28px', position: 'relative' }}>
          <button
            onClick={onClose}
            style={{
              position: 'absolute', top: 14, right: 14,
              width: 28, height: 28, border: 0, borderRadius: 999,
              background: 'transparent', color: 'var(--ol-ink-3)',
              display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
              cursor: 'default',
            }}
            title="关闭">

            <Icon name="close" size={14} />
          </button>

          <h2 style={{ margin: '0 0 18px', fontSize: 22, fontWeight: 600, letterSpacing: '-0.02em' }}>{section}</h2>

          {section === '设置' && <SettingsContent embedded initialSection={initialSettingsSection} />}
          {section === '账户' && <AccountSection />}
          {section === '个性化' && <PersonalizeSection />}
          {section === '关于' && <AboutMini />}
        </div>
      </div>

      <style>{`
        @keyframes ol-modal-fade { from { opacity: 0 } to { opacity: 1 } }
        @keyframes ol-modal-pop {
          from { opacity: 0; transform: translateY(6px) scale(.98); }
          to   { opacity: 1; transform: translateY(0) scale(1); }
        }
      `}</style>
    </div>
  );
}

function AccountSection() {
  return (
    <div>
      <div style={{
        padding: 16, borderRadius: 12,
        border: '0.5px solid var(--ol-line)',
        display: 'flex', alignItems: 'center', gap: 14,
      }}>
        <div style={{
          width: 44, height: 44, borderRadius: 999,
          background: 'linear-gradient(135deg, #0a0a0b, #2563eb)',
          color: '#fff', fontSize: 16, fontWeight: 600,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}>L</div>
        <div style={{ flex: 1 }}>
          <div style={{ fontSize: 14, fontWeight: 600 }}>本地用户</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>未登录 · 所有数据保存在本机</div>
        </div>
        <button style={{
          padding: '7px 14px', fontSize: 12.5, fontWeight: 500,
          borderRadius: 8, border: 0, background: 'var(--ol-ink)', color: '#fff',
          cursor: 'default', fontFamily: 'inherit',
        }}>登录 / 同步</button>
      </div>
      <p style={{ margin: '20px 0 0', fontSize: 12, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
        OpenLess 默认完全本地运行。登录后可在多设备间同步词汇表与风格预设，识别仍在本机或你配置的 Provider 上完成。
      </p>
    </div>
  );
}

function PersonalizeSection() {
  // 玻璃强度持久化到 localStorage，并实时写入 CSS var --ol-glass-blur。
  // 这是 CSS-only 的层（影响 backdrop-filter 的内层强度）；macOS NSVisualEffectView
  // 是另一层，由 Tauri 在窗口创建时一次性配置，运行时改动需要重启 App。
  const [blur, setBlur] = useState<number>(() => {
    const saved = window.localStorage.getItem('ol.glassBlur');
    return saved ? Number(saved) : 22;
  });

  useEffect(() => {
    document.documentElement.style.setProperty('--ol-glass-blur', `${blur}px`);
    window.localStorage.setItem('ol.glassBlur', String(blur));
  }, [blur]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <Row label="外观" desc="跟随系统 / 浅色 / 深色">
        <SegSimple options={['跟随系统', '浅色', '深色']} active="跟随系统" />
      </Row>
      <Row label="界面语言">
        <SelectLite value="简体中文（中国大陆）" />
      </Row>
      <Row label="毛玻璃强度" desc="影响窗口内层 backdrop-filter 强度（macOS 系统磨砂层无法运行时调）。">
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 10 }}>
          <input
            type="range"
            min="0"
            max="48"
            value={blur}
            onChange={e => setBlur(Number(e.target.value))}
            style={{ width: 200, accentColor: 'var(--ol-blue)' }}
          />
          <span style={{ fontSize: 12, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)', minWidth: 36 }}>
            {blur}px
          </span>
        </div>
      </Row>
      <Row label="启动时打开">
        <SegSimple options={['概览', '上次位置']} active="上次位置" />
      </Row>
      <Row label="开机自启">
        <SwitchLite on />
      </Row>
    </div>
  );
}

function AboutMini() {
  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 16 }}>
        <img src="AppIcon.png" alt="" style={{ width: 56, height: 56, borderRadius: 13, boxShadow: '0 4px 10px rgba(0,0,0,.10), 0 0 0 0.5px rgba(0,0,0,.06)' }} />
        <div>
          <div style={{ fontSize: 17, fontWeight: 600 }}>OpenLess</div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>自然说话，完美书写 · {APP_VERSION_LABEL}</div>
        </div>
      </div>
      <Row label="检查更新"><button style={btnGhost}>检查</button></Row>
      <Row label="文档"><button style={btnGhost}>openless.app/docs ↗</button></Row>
      <Row label="反馈渠道"><button style={btnGhost}>GitHub Issues ↗</button></Row>
      <Row label="隐私" desc="所有识别结果只保存在本机，云端 API 仅用于实时调用。">
        <span style={{ fontSize: 11, padding: '3px 8px', borderRadius: 999, background: 'var(--ol-blue-soft)', color: 'var(--ol-blue)', fontWeight: 500 }}>本地优先</span>
      </Row>
    </div>
  );
}

const btnGhost: CSSProperties = {
  padding: '5px 10px', fontSize: 12, borderRadius: 6,
  border: '0.5px solid var(--ol-line-strong)',
  background: '#fff', color: 'var(--ol-ink-2)',
  cursor: 'default', fontFamily: 'inherit',
};
