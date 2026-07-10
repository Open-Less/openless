// Kbd.tsx — 键帽显示（shadcn Kbd 的纯 React 版，用户拍板的快捷键展示标准）。
// 语义用真 <kbd> 元素；键帽视觉走主题 token（浅/深色自适应），底边阴影出立体感。

import type { CSSProperties, ReactNode } from 'react';

export function Kbd({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return <kbd style={{ ...kbdStyle, ...style }}>{children}</kbd>;
}

/** 组合键：一组键帽并排（shadcn KbdGroup），如「左 ⌥」「Space」。 */
export function KbdGroup({ keys, style }: { keys: string[]; style?: CSSProperties }) {
  return (
    <span style={{ ...groupStyle, ...style }}>
      {keys.map((key, i) => (
        <Kbd key={`${key}-${i}`}>{key}</Kbd>
      ))}
    </span>
  );
}

const kbdStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  minWidth: 20,
  height: 21,
  padding: '0 6px',
  borderRadius: 5,
  fontSize: 11.5,
  lineHeight: 1,
  fontWeight: 500,
  fontFamily: 'var(--ol-font-sans)',
  color: 'var(--ol-ink-2)',
  background: 'var(--ol-surface-2)',
  border: '0.5px solid var(--ol-line-strong)',
  // 键帽立体感：底边多一线阴影。
  boxShadow: '0 1.5px 0 var(--ol-line), 0 0 0 0.5px rgba(0,0,0,0.02)',
  whiteSpace: 'nowrap',
};

const groupStyle: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 4,
};
