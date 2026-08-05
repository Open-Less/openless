// 共享在设置各 section 间的原子（SettingRow / Toggle / inputStyle）。
// AsrPresetId 也放在这里，让 settings/ 下各 section 都从同一处来源拿。

import type { CSSProperties, ReactNode } from "react"
import { Tooltip } from "../../components/Tooltip"
import { useMobileLayout } from "../../lib/useMobileLayout"

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
                gridTemplateColumns: mobile ? "minmax(0, 1fr)" : "minmax(0, 180px) minmax(0, 1fr)",
                gap: mobile ? 8 : 16,
                padding: mobile ? "12px 0" : "14px 0",
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

// ASR provider preset 清单 —— 单一来源，放这里让 ProvidersSection /
// LocalModelSection / Overview 都从同一处取，不再各维护一份 id 列表导致漂移。
// `AsrPresetId` 由此派生（与 ProvidersSection 的 LLM_PRESETS→LlmPresetId 同构）。
//
// 新增兼容厂商：
//   1. 这里加一项 `{ id, nameKey, baseUrl, model }`；
//   2. 后端 `coordinator.rs::active_asr_provider_kind` 加 id→kind 映射 —— 之后
//      `preflight_credential` / `configured_fields` 等穷尽 match 会被编译器逐个
//      报错逼你补齐；走 Whisper 协议再加进 `is_whisper_compatible_provider`，
//      专有协议另配独立 ASR client 与 provider kind；
//   3. i18n 的 `settings.providers.presets.<nameKey>` 补各语言文案。
export const ASR_PRESETS = [
  { id: 'volcengine',   nameKey: 'asrVolcengine',   baseUrl: '',                                              model: ''                              },
  { id: 'elevenlabs',   nameKey: 'asrElevenLabs',   baseUrl: 'https://api.elevenlabs.io/v1',                  model: 'scribe_v2'                     },
  { id: 'bailian',      nameKey: 'asrBailian',     baseUrl: 'wss://dashscope.aliyuncs.com/api-ws/v1/inference/', model: 'fun-asr-realtime'             },
  // Qwen3-ASR-Flash 实时：OpenAI Realtime 风格 WS（/api-ws/v1/realtime），
  // 与上面经典 inference 协议不同，由 asr/qwen_realtime.rs 专用 client 处理。
  // 业务空间专属域名（wss://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime）同样可用。
  { id: 'bailian-qwen3-realtime', nameKey: 'asrBailianQwen3', baseUrl: 'wss://dashscope.aliyuncs.com/api-ws/v1/realtime', model: 'qwen3-asr-flash-realtime' },
  // Fun-ASR-Flash 录音文件识别：非实时，走 DashScope 私有的
  // multimodal-generation HTTP 接口（既非实时 WS，也非 OpenAI /audio/transcriptions），
  // 由 asr/dashscope_multimodal.rs 专用批量 client 处理。API key 与百炼同一把。
  { id: 'bailian-fun-asr-flash', nameKey: 'asrBailianFunAsrFlash', baseUrl: 'https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation', model: 'fun-asr-flash-2026-06-15' },
  { id: 'siliconflow',  nameKey: 'asrSiliconflow',  baseUrl: 'https://api.siliconflow.cn/v1',                  model: 'FunAudioLLM/SenseVoiceSmall' },
  // 阶跃星辰 StepAudio：一个入口双协议（照百炼先例，后端按模型名二次路由）。
  // 默认批式模型走 Whisper 兼容 multipart，词典经一等 hotwords 参数生效；
  // 模型改成 stepaudio-2.5-asr-stream 则走实时 WS（asr/stepfun_realtime.rs），
  // 词典改经 transcription.prompt 生效。endpoint 一律填 https base，
  // wss URL 由后端自动派生。
  { id: 'stepfun',      nameKey: 'asrStepfun',      baseUrl: 'https://api.stepfun.com/v1',                     model: 'stepaudio-2.5-asr'           },
  { id: 'zhipu',        nameKey: 'asrZhipu',        baseUrl: 'https://open.bigmodel.cn/api/paas/v4',           model: 'glm-asr-2512'                },
  { id: 'groq',         nameKey: 'asrGroq',         baseUrl: 'https://api.groq.com/openai/v1',                 model: 'whisper-large-v3-turbo'      },
  { id: 'whisper',      nameKey: 'asrWhisper',      baseUrl: 'https://api.openai.com/v1',                      model: 'whisper-1'                   },
  // OpenRouter 的 /audio/transcriptions 走 application/json + base64（issue #582），
  // 后端 coordinator.rs::whisper_request_format 对该 id 切换到 OpenRouterJson 编码。
  { id: 'openrouter',   nameKey: 'asrOpenrouter',   baseUrl: 'https://openrouter.ai/api/v1',                   model: 'openai/whisper-large-v3-turbo' },
  // ZenMux 聚合平台的 /audio/transcriptions 同为 application/json + base64
  // （issue #837），后端 whisper_request_format 对该 id 切 ZenMuxJson；
  // 语言跟随工作语言，enable_itn 开关见 AsrAdvancedOptions（仅该预设显示）。
  { id: 'zenmux',       nameKey: 'asrZenmux',       baseUrl: 'https://zenmux.ai/api/v1',                      model: 'qwen/qwen3-asr-flash'          },
  // 通用 OpenAI 兼容端点（自建 / 局域网 llama.cpp 等）：无默认 endpoint/model，
  // 用户必填；verbose_json 与分片时长由高级选项按 provider 配置（见
  // ProvidersSection 的 AsrAdvancedOptions），默认行为最保守。
  { id: 'openai-compatible', nameKey: 'asrOpenAiCompatible', baseUrl: '',                              model: ''                              },
  // 小米 MiMo ASR 按官方文档走 /chat/completions + input_audio，不是
  // Whisper /audio/transcriptions；后端由 asr/mimo.rs 专用 client 处理。
  { id: 'xiaomi-mimo-asr', nameKey: 'asrXiaomiMimo', baseUrl: 'https://api.xiaomimimo.com/v1',                  model: 'mimo-v2.5-asr'               },
  // 讯飞实时语音转写（RTASR）流式：无 baseUrl/model 配置，凭据是 AppID + APIKey
  // 两字段（asr/xfyun.rs）；音频 16k/16bit/mono，与 recorder 输出一致。
  { id: 'iflytek',      nameKey: 'asrIflytek',      baseUrl: '',                                              model: ''                              },
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper', baseUrl: '',                              model: ''                              },
  // 本地引擎（Foundry / sherpa-onnx / Qwen3）：无 baseUrl/model 配置，
  // 模型在「高级 → 本地模型」里下载与切换。
  { id: 'sherpa-onnx-local',     nameKey: 'asrSherpaOnnxLocal',     baseUrl: '',                              model: ''                              },
  { id: 'local-qwen3',  nameKey: 'asrLocalQwen3',   baseUrl: '',                                              model: ''                              },
  // Apple 系统语音识别（macOS）：无 baseUrl/model、无下载、无凭据。
  { id: 'apple-speech', nameKey: 'asrAppleSpeech',  baseUrl: '',                                              model: ''                              },
] as const;

export type AsrPresetId = typeof ASR_PRESETS[number]['id'];
