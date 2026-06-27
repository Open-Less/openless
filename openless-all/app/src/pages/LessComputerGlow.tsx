// Less Computer 全屏彩虹边缘亮条（独立窗口 window=less-computer-glow）。
// 只画贴边光带，不铺暗场；彩色光环沿边缘流动，模拟 Apple Intelligence 的发光描边。
// 纯视觉：pointer-events:none，后端再 set_ignore_cursor_events(true)。仅 macOS 显示。
//
// 发光技法移植自 EdgeGlow（github.com/vector4wang/EdgeGlow，MIT）：核心是「4 层模糊堆叠」——
// 最内一条 0 模糊的锐利彩虹实线，外面叠 3 层逐渐变宽、变虚的光晕，得到霓虹灯管式发光。
// 配色采用 EdgeGlow 的 iridescent（Apple Intelligence 高饱和霓虹）主题。

import { useEffect, useState } from 'react';

// EdgeGlow iridescent 主题（紫→蓝→青→绿→金→橙→红→粉→紫），沿环 360° 均匀铺开。
const SPECTRUM = `conic-gradient(from var(--lcg-angle),
  #a633f2 0deg,
  #594dff 30deg,
  #2680ff 60deg,
  #0db3fa 90deg,
  #1ad9e6 120deg,
  #33e6bf 150deg,
  #80d966 180deg,
  #ccb31a 210deg,
  #ff801a 240deg,
  #ff4059 270deg,
  #f22699 300deg,
  #bf40d9 330deg,
  #a633f2 360deg)`;

const glowCss = `
@property --lcg-angle { syntax: '<angle>'; initial-value: 0deg; inherits: false; }
@keyframes lcg-spin     { to { --lcg-angle: 360deg; } }
@keyframes lcg-breathe  { 0%, 100% { opacity: var(--lcg-o); } 50% { opacity: calc(var(--lcg-o) + .14); } }

html, body, #root { background: transparent !important; margin: 0; height: 100%; overflow: hidden; }

/* 全屏裁剪容器：圆角贴合屏幕物理圆角；overflow:hidden 把外溢模糊裁在屏幕边缘内侧。 */
.lcg-root {
  position: fixed;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
  border-radius: var(--lcg-radius, 42px);
}

/* 4 层都是同一个「边框环」：用 mask-composite 只保留 padding 那一圈，padding 即环的粗细。 */
.lcg-line,
.lcg-glow-1,
.lcg-glow-2,
.lcg-glow-3 {
  position: absolute;
  inset: -1px;
  border-radius: calc(var(--lcg-radius, 42px) + 1px);
  background: ${SPECTRUM};
  -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
          mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
  -webkit-mask-composite: xor;
          mask-composite: exclude;
  pointer-events: none;
  will-change: filter, opacity, --lcg-angle;
}

/* 第 4 层 → 最细的彩虹实线（EdgeGlow blur=0 的那层）：几乎不模糊，颜色最实，定义边缘。 */
.lcg-line {
  --lcg-o: 1;
  padding: 2.5px;
  filter: saturate(1.32) brightness(1.06) blur(.5px)
    drop-shadow(0 0 4px rgba(150, 120, 255, .55));
  opacity: 1;
  animation: lcg-spin 8s linear infinite;
}

/* 第 3 层 → 内层亮边（EdgeGlow blur≈2）。 */
.lcg-glow-1 {
  --lcg-o: .85;
  padding: 5px;
  filter: saturate(1.28) brightness(1.05) blur(3px);
  opacity: .85;
  mix-blend-mode: screen;
  animation: lcg-spin 8s linear infinite, lcg-breathe 4.6s ease-in-out infinite;
}

/* 第 2 层 → 中层光晕（EdgeGlow blur≈8）。 */
.lcg-glow-2 {
  --lcg-o: .56;
  padding: 11px;
  filter: saturate(1.22) brightness(1.04) blur(8px);
  opacity: .56;
  mix-blend-mode: screen;
  animation: lcg-spin 8s linear infinite, lcg-breathe 5.6s ease-in-out infinite;
}

/* 第 1 层 → 最宽外晕（EdgeGlow blur≈12），向屏内柔和散开。 */
.lcg-glow-3 {
  --lcg-o: .4;
  padding: 20px;
  filter: saturate(1.18) brightness(1.03) blur(15px);
  opacity: .4;
  mix-blend-mode: screen;
  animation: lcg-spin 8s linear infinite, lcg-breathe 6.8s ease-in-out infinite;
}

@media (prefers-reduced-motion: reduce) {
  .lcg-line,
  .lcg-glow-1,
  .lcg-glow-2,
  .lcg-glow-3 { animation: none; }
}
`;

if (typeof document !== 'undefined' && !document.getElementById('less-computer-glow-style')) {
  const tag = document.createElement('style');
  tag.id = 'less-computer-glow-style';
  tag.textContent = glowCss;
  document.head.appendChild(tag);
}

export function LessComputerGlow() {
  // issue #470：窗口 .hide() 后 webview 不会自动停掉这些无限动画（Windows 尤其不释放），
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
      <span className="lcg-glow-3" />
      <span className="lcg-glow-2" />
      <span className="lcg-glow-1" />
      <span className="lcg-line" />
    </div>
  );
}
