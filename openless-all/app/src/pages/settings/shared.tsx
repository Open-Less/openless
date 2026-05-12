// 共享在 Settings 各 section 间的原子（SettingRow / Toggle / inputStyle）。
// AsrPresetId 也放在这里，让 AdvancedSection 与 Settings.tsx 都从一处来源拿。

import type { CSSProperties, ReactNode } from 'react';

interface SettingRowProps {
  label: string;
  desc?: string;
  children: ReactNode;
  controlWidth?: number | string;
}

export function SettingRow({ label, desc, children, controlWidth }: SettingRowProps) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'minmax(0, 180px) minmax(0, 1fr)', gap: 16, padding: '14px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
      </div>
      <div style={{ display: 'flex', alignItems: 'flex-start', minWidth: 0, width: controlWidth ?? 'auto' }}>{children}</div>
    </div>
  );
}

export function Toggle({ on, onToggle }: { on: boolean; onToggle?: (next: boolean) => void }) {
  return (
    <button
      onClick={() => onToggle?.(!on)}
      style={{
        position: 'relative', width: 32, height: 18, borderRadius: 999, border: 0,
        background: on ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
        cursor: 'default',
        transition: 'background 0.16s var(--ol-motion-quick)',
      }}
    >
      <span
        style={{
          position: 'absolute', top: 2, left: on ? 16 : 2,
          width: 14, height: 14, borderRadius: 999, background: '#fff',
          boxShadow: '0 1px 2px rgba(0,0,0,.25)', transition: 'left .16s var(--ol-motion-spring)',
        }}
      />
    </button>
  );
}

export const inputStyle: CSSProperties = {
  flex: 1, height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, fontSize: 12.5,
  fontFamily: 'inherit', outline: 'none',
  background: 'var(--ol-surface-2)',
  width: '100%', maxWidth: 360,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)',
};

// ASR provider id 集合，跟 Settings.tsx::ASR_PRESETS 一一对应。
// 拆成独立类型让 AdvancedSection / ProvidersSection 都能用同一份不互相依赖。
export type AsrPresetId =
  | 'volcengine'
  | 'bailian'
  | 'siliconflow'
  | 'zhipu'
  | 'groq'
  | 'whisper'
  | 'foundry-local-whisper'
  | 'local-qwen3';
