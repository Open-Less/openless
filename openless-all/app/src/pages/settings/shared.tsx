// 共享在设置各 section 间的原子（SettingRow / Toggle / inputStyle）和纯 i18n 标签。

import type { CSSProperties, ReactNode } from "react"
import { Tooltip } from "../../components/Tooltip"
import { useMobileLayout, useReadableLayout, useConservativeLayout } from "../../lib/useMobileLayout"

// 带说明的文字统一加虚线下划线 + help 光标，暗示「悬停可看解释」。
const hintableTextStyle: CSSProperties = {
    cursor: "help",
    textDecoration: "underline dotted",
    textDecorationColor: "var(--ol-ink-4)",
    textUnderlineOffset: 3,
}

export function SectionTitle({
    children,
    hint,
    style,
}: {
    children: ReactNode
    /** 悬停在标题文字上时的功能说明，给 Less Computer 这类光看名字猜不出用途的板块。 */
    hint?: string
    style?: CSSProperties
}) {
    const titleStyle: CSSProperties = {
        fontSize: 14,
        fontWeight: 600,
        color: "var(--ol-ink)",
        marginBottom: 6,
        letterSpacing: "-0.01em",
        ...style,
    }
    if (!hint) {
        return <div style={titleStyle}>{children}</div>
    }
    return (
        // display:flex 让 Tooltip 的锚点收缩到标题文字本身，提示贴着文字弹出。
        <div style={{ ...titleStyle, display: "flex" }}>
            <Tooltip content={hint} wrap placement="bottom" focusable>
                <span style={hintableTextStyle}>{children}</span>
            </Tooltip>
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

// 页面瘦身后描述小字不再常驻展示；desc 改为悬停在标签文字上时以 Tooltip 弹出，
// 布局保持紧凑的同时不牺牲可理解性。
export function SettingRow({
    label,
    desc,
    children,
    controlWidth,
}: SettingRowProps) {
    const mobile = useMobileLayout()
    const readable = useReadableLayout()
    const conservative = useConservativeLayout()
    const stackLayout = mobile || readable || conservative
    const labelStyle: CSSProperties = {
        fontSize: 13,
        fontWeight: 500,
        color: "var(--ol-ink)",
        minWidth: 0,
    }
    return (
        <div
            style={{
                display: "grid",
                gridTemplateColumns: stackLayout ? "minmax(0, 1fr)" : "minmax(0, 180px) minmax(0, 1fr)",
                gap: stackLayout ? 8 : 16,
                padding: stackLayout ? "12px 0" : "14px 0",
                borderTop: "0.5px solid var(--ol-line-soft)",
                alignItems: "center",
            }}
        >
            {/* display:flex 让 Tooltip 锚点收缩到文字宽度，提示贴着文字弹出。 */}
            <div style={{ minWidth: 0, alignSelf: "center", display: "flex" }}>
                {desc ? (
                    <Tooltip content={desc} wrap placement="bottom" focusable>
                        <span style={{ ...labelStyle, ...hintableTextStyle }}>{label}</span>
                    </Tooltip>
                ) : (
                    <div style={labelStyle}>{label}</div>
                )}
            </div>
            <div
                className="ol-flex-row"
                style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "flex-start",
                    minWidth: 0,
                    width: stackLayout ? "100%" : controlWidth ?? "auto",
                    maxWidth: "100%",
                    flexWrap: stackLayout ? "wrap" : "nowrap",
                    gap: stackLayout ? 6 : undefined,
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
                flex: "0 0 36px",
                width: 36,
                minWidth: 36,
                maxWidth: 36,
                height: 20,
                borderRadius: 999,
                border: 0,
                background: on ? "var(--ol-blue)" : "var(--ol-toggle-off-bg)",
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
                    background: "var(--ol-toggle-knob)",
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
    // 与 SelectLite 触发器同底色：此前用 --ol-surface-2（浅灰）会让所有输入框/
    // 下拉与其它设置控件（麦克风/胶囊样式等 select-trigger-bg）颜色不一致。
    background: "var(--ol-select-trigger-bg)",
    width: "100%",
    maxWidth: 360,
    transition:
        "background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick)",
}

// React 只保留展示标签。endpoint、model、auth、probe 与能力全部来自
// Core ProviderDescriptor，避免任一 Host 再拥有一份会漂移的 provider 策略。
// 这里的顺序和键仅用于本地化回退；协议说明也应写在 Core provider_rules 附近。
export const ASR_LABELS = [
  { id: 'volcengine', nameKey: 'asrVolcengine' },
  { id: 'elevenlabs', nameKey: 'asrElevenLabs' },
  { id: 'bailian', nameKey: 'asrBailian' },
  { id: 'bailian-qwen3-realtime', nameKey: 'asrBailianQwen3' },
  { id: 'bailian-fun-asr-flash', nameKey: 'asrBailianFunAsrFlash' },
  { id: 'siliconflow', nameKey: 'asrSiliconflow' },
  { id: 'stepfun', nameKey: 'asrStepfun' },
  { id: 'zhipu', nameKey: 'asrZhipu' },
  { id: 'groq', nameKey: 'asrGroq' },
  { id: 'whisper', nameKey: 'asrWhisper' },
  { id: 'openrouter', nameKey: 'asrOpenrouter' },
  { id: 'zenmux', nameKey: 'asrZenmux' },
  { id: 'openai-compatible', nameKey: 'asrOpenAiCompatible' },
  { id: 'xiaomi-mimo-asr', nameKey: 'asrXiaomiMimo' },
  { id: 'iflytek', nameKey: 'asrIflytek' },
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper' },
  { id: 'local-whisper', nameKey: 'asrLocalWhisper' },
  { id: 'sherpa-onnx-local', nameKey: 'asrSherpaOnnxLocal' },
  { id: 'local-qwen3-mlx', nameKey: 'asrLocalQwen3Mlx' },
  { id: 'local-qwen3-c', nameKey: 'asrLocalQwen3C' },
  { id: 'local-qwen3', nameKey: 'asrLocalQwen3' },
  { id: 'apple-speech', nameKey: 'asrAppleSpeech' },
] as const;
