// variants.jsx — frosted outer frame + raised inner console.
// Sidebar lives INSIDE the console card. Footer icons sit on the frosted outer.
// Settings is no longer a sidebar tab — it opens as a centered modal sheet.

const { Overview, History, Vocab, Style, Settings: SettingsContent } = window.OLPages;

const NAV = [
{ id: 'overview', name: '概览', icon: 'overview', cmp: Overview },
{ id: 'history', name: '历史', icon: 'history', cmp: History },
{ id: 'vocab', name: '词汇表', icon: 'vocab', cmp: Vocab },
{ id: 'style', name: '风格', icon: 'style', cmp: Style }];


const FloatingShell = ({ os = 'mac', initialTab = 'overview', initialSettings = false }) => {
  const [tab, setTab] = React.useState(initialTab);
  const [settingsOpen, setSettingsOpen] = React.useState(initialSettings);
  const Page = NAV.find((n) => n.id === tab).cmp;

  return (
    <div style={{ flex: 1, position: 'relative', display: 'flex', flexDirection: 'column', minHeight: 0, paddingTop: os === 'mac' ? 36 : 0 }}>

      {/* Main shell — flush with the frosted backplate (no separate float). */}
      <div
        style={{
          flex: 1, minHeight: 0,
          display: 'flex',
          background: 'transparent',
          overflow: 'hidden',
          position: 'relative',
          zIndex: 1
        }}>
        
        {/* Sidebar — inside the raised console */}
        <aside
          style={{
            width: 188,
            flexShrink: 0,
            display: 'flex', flexDirection: 'column',
            background: 'linear-gradient(180deg, rgba(247,247,250,0.85) 0%, rgba(247,247,250,0.5) 100%)',
            padding: '14px 10px 12px'
          }}>
          
          {/* brand */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '4px 8px 14px' }}>
            <img
              src="AppIcon.png"
              alt="OpenLess"
              style={{ width: 22, height: 22, borderRadius: 5, boxShadow: '0 1px 2px rgba(0,0,0,.1), 0 0 0 0.5px rgba(0,0,0,.06)' }} />
            
            <div style={{ fontSize: 13.5, fontWeight: 600, letterSpacing: '-0.01em', color: 'var(--ol-ink)' }}>OpenLess</div>
            <span style={{
              marginLeft: 'auto', padding: '1px 6px', fontSize: 9.5, fontWeight: 600,
              borderRadius: 4, background: 'rgba(0,0,0,0.06)', color: 'var(--ol-ink-3)',
              letterSpacing: '0.04em'
            }}>v1.0</span>
          </div>

          {/* nav */}
          <nav style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
            {NAV.map((n) => {
              const active = tab === n.id;
              return (
                <button
                  key={n.id}
                  onClick={() => setTab(n.id)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '7px 10px',
                    borderRadius: 8, border: 0,
                    background: active ? 'var(--ol-surface)' : 'transparent',
                    color: active ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                    fontFamily: 'inherit', fontSize: 13, fontWeight: active ? 600 : 500,
                    boxShadow: active ? '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)' : 'none',
                    cursor: 'default', transition: 'background .12s, color .12s',
                    textAlign: 'left'
                  }}>
                  
                  <Icon name={n.icon} size={14} />
                  <span style={{ flex: 1 }}>{n.name}</span>
                  {n.id === 'history' &&
                  <span style={{
                    fontSize: 10, fontFamily: 'var(--ol-font-mono)',
                    color: active ? 'var(--ol-ink-4)' : 'var(--ol-ink-5)'
                  }}>{OL_DATA.history.length}</span>
                  }
                </button>);

            })}
          </nav>

          <div style={{ flex: 1 }} />

          {/* shortcut hint */}
          <div
            style={{
              padding: '10px 10px 8px',
              borderTop: '0.5px dashed var(--ol-line)',
              marginTop: 8
            }}>
            
            <div style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', marginBottom: 6, letterSpacing: '0.02em' }}>录音快捷键</div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 11, color: 'var(--ol-ink-2)' }}>
              <kbd style={{
                padding: '2px 7px', fontSize: 10.5,
                background: '#fff', borderRadius: 5,
                border: '0.5px solid var(--ol-line-strong)',
                fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink)',
                boxShadow: '0 1px 0 rgba(0,0,0,.04)'
              }}>右 Option</kbd>
              <span style={{ color: 'var(--ol-ink-4)' }}>开始 / 停止</span>
            </div>
          </div>

          {/* trial / status */}
          <div
            style={{
              marginTop: 10,
              padding: 12,
              borderRadius: 10,
              background: 'linear-gradient(160deg, rgba(37,99,235,0.08) 0%, rgba(37,99,235,0.02) 100%)',
              border: '0.5px solid rgba(37,99,235,0.15)'
            }}>
            
            <div style={{ fontSize: 10.5, fontWeight: 600, color: 'var(--ol-blue)', letterSpacing: '0.04em', textTransform: 'uppercase' }}>BETA</div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-2)', marginTop: 4, lineHeight: 1.5 }}>所有数据都只保存在本机。</div>
          </div>
        </aside>

        {/* Main content */}
        {/* Main content — inset white card sitting on the frosted backplate */}
        <div style={{ flex: 1, minWidth: 0, padding: '6px 8px 6px 0', display: 'flex' }}>
          <main
            style={{
              flex: 1, minWidth: 0,
              overflow: 'auto',
              background: 'var(--ol-surface)',
              borderRadius: 12,
              border: '0.5px solid rgba(0,0,0,0.06)',
              boxShadow: '0 1px 0 rgba(255,255,255,0.8) inset, 0 8px 24px -12px rgba(15,17,22,0.10), 0 2px 6px -2px rgba(15,17,22,0.06)',
            }}
          >
            <div style={{ padding: '24px 28px 32px', minHeight: '100%' }}>
              <Page />
            </div>
          </main>
        </div>
      </div>

      {/* Footer — sits on frosted outer, like Typeless */}
      <div
        style={{
          flexShrink: 0,
          height: 44,
          display: 'flex', alignItems: 'center',
          padding: '0 24px',
          gap: 4,
          fontSize: 11,
          color: 'var(--ol-ink-4)',
          position: 'relative',
          zIndex: 2
        }}>
        
        <FooterIcon name="user" tip="账户" />
        <FooterIcon name="mail" tip="反馈" />
        <FooterIcon name="settings" tip="设置" active={settingsOpen} onClick={() => setSettingsOpen(true)} />
        <FooterIcon name="help" tip="帮助" />

        <div style={{ flex: 1 }} />

        <span style={{ fontFamily: 'var(--ol-font-sans)' }}>版本 v1.0.0</span>
        <a style={{ color: 'var(--ol-blue)', marginLeft: 8, textDecoration: 'none', fontWeight: 500 }}>检查更新</a>
      </div>

      {/* Settings modal — rendered inside this window */}
      {settingsOpen &&
      <SettingsModal os={os} onClose={() => setSettingsOpen(false)} />
      }
    </div>);

};

const FooterIcon = ({ name, tip, active, onClick }) =>
<button
  onClick={onClick}
  title={tip}
  style={{
    width: 30, height: 30, borderRadius: 7, border: 0,
    background: active ? 'rgba(0,0,0,0.06)' : 'transparent',
    color: active ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
    display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
    cursor: 'default'
  }}>
  
    <Icon name={name} size={15} />
  </button>;


// ─── Settings Modal — centered sheet, sub-nav on left ─────────────────────
const SettingsModal = ({ os, onClose }) => {
  const [section, setSection] = React.useState('设置');
  const groups = [
  { items: [{ id: '账户', icon: 'user' }, { id: '设置', icon: 'settings' }, { id: '个性化', icon: 'sparkle' }, { id: '关于', icon: 'info' }] },
  { items: [{ id: '帮助中心', icon: 'help', external: true }, { id: '版本说明', icon: 'doc', external: true }] }];


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
        animation: 'ol-modal-fade .18s ease-out'
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
          position: 'relative'
        }}>
        
        {/* sub-sidebar */}
        <aside
          style={{
            width: 200, flexShrink: 0,
            background: 'rgba(247,247,250,0.7)',
            borderRight: '0.5px solid var(--ol-line-soft)',
            padding: '18px 12px',
            display: 'flex', flexDirection: 'column', gap: 14
          }}>
          
          {groups.map((g, gi) =>
          <div key={gi} style={{ display: 'flex', flexDirection: 'column', gap: 1, paddingTop: gi === 1 ? 8 : 0, borderTop: gi === 1 ? '0.5px solid var(--ol-line-soft)' : 'none' }}>
              {g.items.map((it) => {
              const active = section === it.id && !it.external;
              return (
                <button
                  key={it.id}
                  onClick={() => !it.external && setSection(it.id)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '7px 10px',
                    borderRadius: 8, border: 0,
                    background: active ? '#fff' : 'transparent',
                    color: active ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                    fontFamily: 'inherit', fontSize: 13, fontWeight: active ? 600 : 500,
                    boxShadow: active ? '0 1px 2px rgba(0,0,0,.05), 0 0 0 0.5px rgba(0,0,0,.06)' : 'none',
                    cursor: 'default', textAlign: 'left'
                  }}>
                  
                    <Icon name={it.icon} size={14} />
                    <span style={{ flex: 1 }}>{it.id}</span>
                    {it.external && <Icon name="external" size={11} />}
                  </button>);

            })}
            </div>
          )}
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
              cursor: 'default'
            }}
            title="关闭">
            
            <Icon name="close" size={14} />
          </button>

          <h2 style={{ margin: '0 0 18px', fontSize: 22, fontWeight: 600, letterSpacing: '-0.02em' }}>{section}</h2>

          {section === '设置' && <SettingsContent embedded />}
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
    </div>);

};

const AccountSection = () =>
<div>
    <div style={{
    padding: 16, borderRadius: 12,
    border: '0.5px solid var(--ol-line)',
    display: 'flex', alignItems: 'center', gap: 14
  }}>
      <div style={{
      width: 44, height: 44, borderRadius: 999,
      background: 'linear-gradient(135deg, #0a0a0b, #2563eb)',
      color: '#fff', fontSize: 16, fontWeight: 600,
      display: 'flex', alignItems: 'center', justifyContent: 'center'
    }}>L</div>
      <div style={{ flex: 1 }}>
        <div style={{ fontSize: 14, fontWeight: 600 }}>本地用户</div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>未登录 · 所有数据保存在本机</div>
      </div>
      <button style={{
      padding: '7px 14px', fontSize: 12.5, fontWeight: 500,
      borderRadius: 8, border: 0, background: 'var(--ol-ink)', color: '#fff',
      cursor: 'default', fontFamily: 'inherit'
    }}>登录 / 同步</button>
    </div>
    <p style={{ margin: '20px 0 0', fontSize: 12, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
      OpenLess 默认完全本地运行。登录后可在多设备间同步词汇表与风格预设，识别仍在本机或你配置的 Provider 上完成。
    </p>
  </div>;


const PersonalizeSection = () =>
<div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
    <Row label="外观" desc="跟随系统 / 浅色 / 深色">
      <SegSimple options={['跟随系统', '浅色', '深色']} active="跟随系统" />
    </Row>
    <Row label="界面语言">
      <SelectLite value="简体中文（中国大陆）" />
    </Row>
    <Row label="毛玻璃强度" desc="影响窗口外框与底栏的模糊级别">
      <input type="range" min="0" max="48" defaultValue="22" style={{ width: 200, accentColor: 'var(--ol-blue)' }} />
    </Row>
    <Row label="启动时打开">
      <SegSimple options={['概览', '上次位置']} active="上次位置" />
    </Row>
    <Row label="开机自启">
      <SwitchLite on />
    </Row>
  </div>;


const AboutMini = () =>
<div>
    <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 16 }}>
      <img src="AppIcon.png" alt="" style={{ width: 56, height: 56, borderRadius: 13, boxShadow: '0 4px 10px rgba(0,0,0,.10), 0 0 0 0.5px rgba(0,0,0,.06)' }} />
      <div>
        <div style={{ fontSize: 17, fontWeight: 600 }}>OpenLess</div>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>自然说话，完美书写 · v1.0.0 (Build 412)</div>
      </div>
    </div>
    <Row label="检查更新"><button style={btnGhost}>检查</button></Row>
    <Row label="文档"><button style={btnGhost}>openless.app/docs ↗</button></Row>
    <Row label="反馈渠道"><button style={btnGhost}>GitHub Issues ↗</button></Row>
    <Row label="隐私" desc="所有识别结果只保存在本机，云端 API 仅用于实时调用。">
      <span style={{ fontSize: 11, padding: '3px 8px', borderRadius: 999, background: 'var(--ol-blue-soft)', color: 'var(--ol-blue)', fontWeight: 500 }}>本地优先</span>
    </Row>
  </div>;


const Row = ({ label, desc, children }) =>
<div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: 16, padding: '12px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
    <div>
      <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
      {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
    </div>
    <div style={{ display: 'flex', alignItems: 'center' }}>{children}</div>
  </div>;

const SegSimple = ({ options, active: a }) => {
  const [v, setV] = React.useState(a);
  return (
    <div style={{ display: 'inline-flex', padding: 2, borderRadius: 8, background: 'rgba(0,0,0,0.05)' }}>
      {options.map((o) =>
      <button key={o} onClick={() => setV(o)} style={{
        padding: '5px 12px', fontSize: 12, fontWeight: 500, border: 0, borderRadius: 6,
        fontFamily: 'inherit',
        background: v === o ? '#fff' : 'transparent',
        color: v === o ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
        boxShadow: v === o ? '0 1px 2px rgba(0,0,0,.08)' : 'none',
        cursor: 'default'
      }}>{o}</button>
      )}
    </div>);

};
const SelectLite = ({ value }) =>
<div style={{
  display: 'inline-flex', alignItems: 'center', gap: 8,
  padding: '6px 10px', fontSize: 12.5,
  borderRadius: 8, border: '0.5px solid var(--ol-line-strong)',
  background: 'var(--ol-surface-2)',
  minWidth: 200, justifyContent: 'space-between'
}}>
    <span>{value}</span>
    <Icon name="chevDown" size={11} />
  </div>;

const SwitchLite = ({ on: i = false }) => {
  const [on, setOn] = React.useState(i);
  return (
    <button onClick={() => setOn(!on)} style={{
      position: 'relative', width: 32, height: 18, borderRadius: 999, border: 0,
      background: on ? 'var(--ol-blue)' : 'rgba(0,0,0,0.18)',
      cursor: 'default'
    }}>
      <span style={{
        position: 'absolute', top: 2, left: on ? 16 : 2,
        width: 14, height: 14, borderRadius: 999, background: '#fff',
        boxShadow: '0 1px 2px rgba(0,0,0,.25)', transition: 'left .15s'
      }} />
    </button>);

};
const btnGhost = {
  padding: '5px 10px', fontSize: 12, borderRadius: 6,
  border: '0.5px solid var(--ol-line-strong)',
  background: '#fff', color: 'var(--ol-ink-2)',
  cursor: 'default', fontFamily: 'inherit'
};

window.FloatingShell = FloatingShell;
window.SettingsModal = SettingsModal;