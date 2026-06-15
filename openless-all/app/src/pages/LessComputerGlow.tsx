// Less Computer 全屏彩虹边缘亮条（独立窗口 window=less-computer-glow）。
// 只画贴边光带，不铺暗场；彩色弧段沿边缘流动，模拟 Apple Intelligence 的粗细变化。
// 纯视觉：pointer-events:none，后端再 set_ignore_cursor_events(true)。仅 macOS 显示。

import { useEffect, useState } from 'react';

const glowCss = `
@property --lcg-angle { syntax: '<angle>'; initial-value: 0deg; inherits: false; }
@keyframes lcg-spin    { to { --lcg-angle: 360deg; } }
@keyframes lcg-breathe { 0%, 100% { opacity: .72; } 50% { opacity: .92; } }
@keyframes lcg-flow    { 0%, 100% { opacity: .44; } 48% { opacity: .74; } }

html, body, #root { background: transparent !important; margin: 0; height: 100%; overflow: hidden; }

/* 全屏裁剪容器：圆角贴合屏幕物理圆角；overflow:hidden 把外溢模糊裁在屏幕边缘。 */
.lcg-root {
  --lcg-spectrum: conic-gradient(from calc(var(--lcg-angle) - 74deg),
    #4e9dff 0deg,
    #6cc9ff 40deg,
    #9d82ff 82deg,
    #e77dff 124deg,
    #ff7aa8 162deg,
    #ff9765 198deg,
    #ffe070 236deg,
    #bff47a 266deg,
    #63e8a2 304deg,
    #63d4ff 334deg,
    #4e9dff 360deg);
  position: fixed;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
  border-radius: var(--lcg-radius, 42px);
}

.lcg-edge,
.lcg-flow {
  position: absolute;
  -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
          mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor;
          mask-composite: exclude;
  pointer-events: none;
  will-change: transform, filter, opacity, --lcg-angle;
}

/* 贴边主光带：只保留边缘亮条，避免在屏幕中央铺暗场或彩雾。 */
.lcg-edge {
  inset: -4px;
  border-radius: calc(var(--lcg-radius, 42px) + 4px);
  padding: 12px;
  background: var(--lcg-spectrum);
  filter: blur(1.1px) saturate(1.36) brightness(1.08)
    drop-shadow(0 0 7px rgba(95, 185, 255, .44))
    drop-shadow(0 0 10px rgba(255, 126, 168, .30));
  opacity: .84;
  animation: lcg-spin 7.5s linear infinite, lcg-breathe 4.8s ease-in-out infinite;
}

/* 彩色粗细流动层：仍然是边缘 ring，不向中间铺开。 */
.lcg-flow {
  inset: -7px;
  border-radius: calc(var(--lcg-radius, 42px) + 7px);
  padding: 18px;
  background: conic-gradient(from calc(var(--lcg-angle) + 28deg),
    rgba(31,140,255,0) 0deg,
    rgba(91,166,255,.74) 28deg,
    rgba(167,134,255,.58) 54deg,
    rgba(240,92,255,0) 82deg,
    rgba(240,92,255,0) 132deg,
    rgba(255,138,94,.70) 164deg,
    rgba(255,220,103,.52) 192deg,
    rgba(217,255,63,0) 222deg,
    rgba(217,255,63,0) 266deg,
    rgba(100,232,164,.68) 294deg,
    rgba(93,210,255,.56) 326deg,
    rgba(31,140,255,0) 360deg);
  filter: blur(4.5px) saturate(1.42) brightness(1.08);
  opacity: .58;
  mix-blend-mode: screen;
  animation: lcg-spin 6.8s linear infinite reverse, lcg-flow 3.8s ease-in-out infinite;
}

@media (prefers-reduced-motion: reduce) {
  .lcg-edge,
  .lcg-flow { animation: none; }
}
`;

if (typeof document !== 'undefined' && !document.getElementById('less-computer-glow-style')) {
  const tag = document.createElement('style');
  tag.id = 'less-computer-glow-style';
  tag.textContent = glowCss;
  document.head.appendChild(tag);
}

export function LessComputerGlow() {
  // issue #470：窗口 .hide() 后 webview 不会自动停掉这 4 条无限动画（Windows 尤其不释放），
  // 全屏发光层持续占 GPU 合成。改由后端 show/hide 主动 emit 可见状态驱动：不可见时直接卸载
  // 发光层（无元素 → 零 GPU），可见时原样渲染——显示时视觉零变化。
  const [active, setActive] = useState(true);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/event').then(({ listen }) => {
      if (cancelled) return;
      void listen<boolean>('less-computer-glow:active', (e) => {
        setActive(Boolean(e.payload));
      }).then((un) => {
        if (cancelled) un();
        else unlisten = un;
      });
    });
    // 二级兜底：页面被标记为隐藏时也停（只停不启，绝不误关正在显示的发光）。
    const onVisibility = () => {
      if (document.hidden) setActive(false);
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      cancelled = true;
      unlisten?.();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, []);

  if (!active) return null;
  return (
    <div className="lcg-root" aria-hidden>
      <span className="lcg-flow" />
      <span className="lcg-edge" />
    </div>
  );
}
