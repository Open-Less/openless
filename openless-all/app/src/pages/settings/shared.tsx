// 共享在设置各 section 间的原子（SettingRow / Toggle / inputStyle）。
// AsrPresetId 也放在这里，让 settings/ 下各 section 都从同一处来源拿。

import type { CSSProperties, ReactNode } from "react"
import { useMobileLayout } from "../../lib/useMobileLayout"

export function SectionTitle({
    children,
    style,
}: {
    children: ReactNode
    style?: CSSProperties
}) {
    return (
        <div
            style={{
                fontSize: 14,
                fontWeight: 600,
                color: "var(--ol-ink)",
                marginBottom: 6,
                letterSpacing: "-0.01em",
                ...style,
            }}
        >
            {children}
        </div>
    )
}

// 页面瘦身：设置页描述文案全部隐藏（保留组件签名 + 调用点，便于需要时恢复）。
export function SectionDesc(_props: {
    children: ReactNode
    style?: CSSProperties
}) {
    return null
}

interface SettingRowProps {
    label: string
    desc?: string
    children: ReactNode
    controlWidth?: number | string
}

// 页面瘦身：不再渲染每行的描述小字（desc 仍保留在 props 里，调用点无需改、便于恢复）。
export function SettingRow({
    label,
    children,
    controlWidth,
}: SettingRowProps) {
    const mobile = useMobileLayout()
    return (
        <div
            style={{
                display: "grid",
                gridTemplateColumns: mobile ? "minmax(0, 1fr)" : "minmax(0, 180px) minmax(0, 1fr)",
                gap: mobile ? 8 : 16,
                padding: mobile ? "12px 0" : "14px 0",
                borderTop: "0.5px solid var(--ol-line-soft)",
                alignItems: "center",
            }}
        >
            <div style={{ minWidth: 0, alignSelf: "center" }}>
                <div
                    style={{
                        fontSize: 13,
                        fontWeight: 500,
                        color: "var(--ol-ink)",
                    }}
                >
                    {label}
                </div>
            </div>
            <div
                style={{
                    display: "flex",
                    alignItems: "center",
                    minWidth: 0,
                    width: mobile ? "100%" : controlWidth ?? "auto",
                    maxWidth: "100%",
                    flexWrap: mobile ? "wrap" : "nowrap",
                    gap: mobile ? 6 : undefined,
                }}
            >
                {children}
            </div>
        </div>
    )
}

export function Toggle({
    on,
    onToggle,
}: {
    on: boolean
    onToggle?: (next: boolean) => void
}) {
    return (
        <button
            onClick={() => onToggle?.(!on)}
            style={{
                position: "relative",
                width: 36,
                height: 20,
                borderRadius: 999,
                border: 0,
                background: on ? "var(--ol-blue)" : "rgba(0,0,0,0.15)",
                boxShadow: "inset 0 1px 2px rgba(0,0,0,0.06)",
                cursor: "default",
                transition: "background 0.16s var(--ol-motion-quick)",
            }}
        >
            <span
                style={{
                    position: "absolute",
                    top: 2,
                    left: on ? 18 : 2,
                    width: 16,
                    height: 16,
                    borderRadius: 999,
                    background: "#fff",
                    boxShadow:
                        "0 1px 2px rgba(0,0,0,.25), 0 0 0 0.5px rgba(0,0,0,.04)",
                    transition: "left .16s var(--ol-motion-spring)",
                }}
            />
        </button>
    )
}

export function chipSelectedStyle(selected: boolean): CSSProperties {
    return {
        background: selected ? "var(--ol-pill-selected-bg)" : "transparent",
        border: selected
            ? "0.5px solid var(--ol-pill-selected-border)"
            : "0.5px solid var(--ol-line-strong)",
        color: selected ? "var(--ol-pill-selected-ink)" : "var(--ol-ink-3)",
    }
}

export const btnGhostStyle: CSSProperties = {
    padding: "5px 10px",
    fontSize: 12,
    borderRadius: 6,
    border: "0.5px solid var(--ol-line-strong)",
    background: "var(--ol-control-solid)",
    color: "var(--ol-ink-2)",
    cursor: "default",
    fontFamily: "inherit",
    maxWidth: "100%",
    transition:
        "background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)",
}

export const segmentedTrackStyle: CSSProperties = {
    display: "inline-flex",
    padding: 2,
    borderRadius: 8,
    background: "var(--ol-segmented-bg)",
}

export const inputStyle: CSSProperties = {
    flex: 1,
    height: 32,
    padding: "0 10px",
    border: "0.5px solid var(--ol-line-strong)",
    borderRadius: 8,
    fontSize: 12.5,
    fontFamily: "inherit",
    outline: "none",
    background: "var(--ol-surface-2)",
    width: "100%",
    maxWidth: 360,
    transition:
        "background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)",
}

// ASR provider id 集合，跟 ProvidersSection.tsx::ASR_PRESETS 一一对应。
// 拆成独立类型让 LocalModelSection / ProvidersSection 都能用同一份不互相依赖。
export type AsrPresetId =
    | "volcengine"
    | "bailian"
    | "siliconflow"
    | "zhipu"
    | "groq"
    | "whisper"
    | "openrouter"
    | "xiaomi-mimo-asr"
    | "foundry-local-whisper"
    | "sherpa-onnx-local"
    | "local-qwen3"
    | "apple-speech"
