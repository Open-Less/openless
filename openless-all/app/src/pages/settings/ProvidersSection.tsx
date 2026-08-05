// 服务 → AI 提供商：LLM 润色模型 + ASR 语音转写两张卡片。
// 自 Settings.tsx 整体迁出，逻辑零改动；i18n key 全部保持 `settings.providers.*`。

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { detectOS } from '../../components/WindowChrome';
import {
  listProviderModels,
  readCredential,
  setActiveAsrProvider,
  setActiveLlmProvider,
  setCredential,
  validateProviderCredentials,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite, type SelectOption } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle, inputStyle, ASR_PRESETS, type AsrPresetId } from './shared';
import {
  parseAdvancedAsrConfig,
  serializeAdvancedAsrConfig,
  type AdvancedAsrConfig,
} from '../../lib/advancedAsrConfig';
import {
  getFoundryLocalAsrCatalog,
  getSherpaOnnxAsrCatalog,
  listLocalAsrModels,
  setFoundryLocalAsrModel,
  setLocalAsrActiveModel,
  setSherpaOnnxAsrModel,
} from '../../lib/localAsr';

// 本地模型供应商：在主下拉里标注「本地」后缀，与云端供应商区分开。
const LOCAL_ASR_PRESET_IDS: ReadonlySet<string> = new Set([
  'local-qwen3',
  'foundry-local-whisper',
  'sherpa-onnx-local',
]);
function isLocalAsrPreset(id: string): boolean {
  return LOCAL_ASR_PRESET_IDS.has(id);
}

function LlmThinkingToggle({ enabled, onToggle }: { enabled: boolean; onToggle: (next: boolean) => void }) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  return (
    <div
      title={t('settings.providers.thinkingModeHint')}
      style={{
        display: 'flex',
        alignItems: 'center',
        flex: mobile ? '1 1 100%' : undefined,
        flexWrap: mobile ? 'wrap' : 'nowrap',
        gap: 6,
        paddingLeft: 2,
        whiteSpace: mobile ? 'normal' : 'nowrap',
      }}
    >
      <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>
        {t('settings.providers.thinkingModeLabel')}
      </span>
      <Toggle on={enabled} onToggle={onToggle} />
      <span style={{ fontSize: 11.5, color: enabled ? 'var(--ol-blue)' : 'var(--ol-ink-4)' }}>
        {enabled ? t('settings.providers.thinkingModeOn') : t('settings.providers.thinkingModeOff')}
      </span>
    </div>
  );
}

export const LLM_PRESETS = [
  {
    id: 'ark',
    nameKey: 'ark',
    baseUrl: 'https://ark.cn-beijing.volces.com/api/v3',
    modelPlaceholder: 'deepseek-v3-2',
  },
  {
    id: 'deepseek',
    nameKey: 'deepseek',
    baseUrl: 'https://api.deepseek.com/v1',
    modelPlaceholder: 'deepseek-v4-flash',
  },
  {
    id: 'siliconflow',
    nameKey: 'siliconflow',
    baseUrl: 'https://api.siliconflow.cn/v1',
    modelPlaceholder: 'Qwen/Qwen2.5-7B-Instruct',
  },
  {
    id: 'atlascloud',
    nameKey: 'atlascloud',
    baseUrl: 'https://api.atlascloud.ai/v1',
    modelPlaceholder: 'qwen/qwen3.5-flash',
  },
  {
    id: 'openai',
    nameKey: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    modelPlaceholder: 'gpt-4o',
  },
  {
    // 谷歌官方 Gemini API（原生 generateContent，不走 OpenAI 兼容 shim）。
    // baseUrl 末尾 /v1beta 是当前 Generally Available 的 path（ai.google.dev/api）。
    // 后端 llm_gemini.rs 会拼成 `{baseUrl}/models/{model}:generateContent`，
    // 并按 Gemini 原生通道级 thinkingConfig 关闭或压低思考，不在前端维护模型适配表。
    // 模型列表用 ProviderTools「拉取模型」按钮取，
    // 由 commands.rs::fetch_provider_models 识别 generativelanguage 域名后按 Gemini shape 解析。
    id: 'gemini',
    nameKey: 'gemini',
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta',
    modelPlaceholder: 'gemini-2.5-flash',
  },
  {
    id: 'codex_oauth',
    nameKey: 'codexOAuth',
    baseUrl: '',
    // gpt-5.3-codex-spark 对 ChatGPT 账号的 Codex 通道会被 400 拒绝，
    // 默认与占位一律用实测可用的 gpt-5.5（见 polish.rs::CODEX_DEFAULT_MODEL）。
    modelPlaceholder: 'gpt-5.5',
  },
  {
    id: 'mimo',
    nameKey: 'mimo',
    baseUrl: 'https://api.xiaomimimo.com/v1',
    modelPlaceholder: 'xiaomi/mimo-v2-flash',
  },
  {
    id: 'cometapi',
    nameKey: 'cometapi',
    baseUrl: 'https://api.cometapi.com/v1',
    modelPlaceholder: 'gpt-4o',
  },
  {
    id: 'openrouterFree',
    nameKey: 'openrouterFree',
    baseUrl: 'https://openrouter.ai/api/v1',
    modelPlaceholder: 'qwen/qwen3-coder:free',
  },
  {
    id: 'alibabaCoding',
    nameKey: 'alibabaCoding',
    baseUrl: 'https://coding-intl.dashscope.aliyuncs.com/v1',
    modelPlaceholder: 'qwen3-coder-plus',
  },
  {
    id: 'codingPlanX',
    nameKey: 'codingPlanX',
    baseUrl: 'https://api.codingplanx.ai/v1',
    modelPlaceholder: 'gpt-5-mini',
  },
  {
    // MiniMax 国内开放平台（minimaxi.com），OpenAI 兼容 /v1/chat/completions。
    // M3 默认开启 thinking，可通过 `thinking.type = disabled` 关闭。
    // provider_id 在后端 polish.rs::openai_compatible_thinking_control 命中
    // "minimax" → MiniMaxThinking 分支，关闭时下发 disabled、开启时发 adaptive。
    // 走"自定义"preset 接入时由 base_url 含 "minimax" 兜底识别,见 polish.rs。
    // 文档: https://platform.minimaxi.com/docs/api-reference/text-chat-openai#thinking-控制
    id: 'minimax',
    nameKey: 'minimax',
    baseUrl: 'https://api.minimaxi.com/v1',
    modelPlaceholder: 'MiniMax-M3',
  },
  {
    // StepFun（阶跃星辰）OpenAI 兼容 /v1/chat/completions。
    // 默认模型选 step-1o-turbo-vision：step-3.x-flash 系列是推理模型且思考无法关闭
    // （reasoning_effort 只能调档，正式内容要等隐藏思考结束，润色场景 TTFT 2s+），
    // 而 step-1o-turbo-vision 无思考、TTFT ~0.3s，润色忠实度实测更适合听写链路。
    // provider_id 在后端 polish.rs::openai_compatible_thinking_control 命中
    // "stepfun" → ReasoningEffort 分支；走"自定义"preset 接入时由 base_url
    // 含 "stepfun" 兜底识别，见 polish.rs。
    id: 'stepfun',
    nameKey: 'stepfun',
    baseUrl: 'https://api.stepfun.com/v1',
    modelPlaceholder: 'step-1o-turbo-vision',
  },
  {
    id: 'custom',
    nameKey: 'custom',
    baseUrl: '',
    modelPlaceholder: '',
  },
] as const;

type LlmPresetId = typeof LLM_PRESETS[number]['id'];

const ASR_DEFAULT_RESOURCE_ID = 'volc.seedasr.sauc.duration';

// ASR_PRESETS 已上移到 settings/shared.tsx 作为单一来源（AsrPresetId 由其派生，
// Overview 的显示名映射也从那里取）。新增厂商的步骤见 shared.tsx 的注释。

// 云端 ASR 模型预设（下拉可选）——千问新发布的 ASR 优先：qwen3-asr-flash 是
// Qwen-ASR 的 OpenAI 兼容 HTTP 形态，另有实时变体（flash-realtime）；fun-asr
// 系列是百炼原生推荐，paraformer 为老一代兜底。
// 注意：qwen3-asr-flash-filetrans 官方只接受公网音频 URL，与本地录音链路不兼容，
// 后端会显式拒绝（coordinator.rs::resolve_effective_asr_provider），不放预设。
const BAILIAN_ASR_MODELS: string[] = [
  'qwen3-asr-flash-realtime',
  'qwen3-asr-flash',
  'fun-asr-realtime',
  'fun-asr',
  'fun-asr-flash-2026-06-15',
  'fun-asr-mtl',
  'paraformer-realtime-v2',
  'paraformer-v2',
];

// OpenAI 兼容（/audio/transcriptions）厂商共用的模型预设。
const OPENAI_COMPAT_ASR_MODELS: string[] = [
  'whisper-large-v3-turbo',
  'whisper-large-v3',
  'whisper-1',
  'FunAudioLLM/SenseVoiceSmall',
  'qwen3-asr-flash',
];

// 走 Whisper 兼容 /audio/transcriptions 协议的厂商（与后端
// coordinator.rs::is_whisper_compatible_provider 保持一致）。其余非百炼厂商
// （zhipu / stepfun / mimo / elevenlabs 等）协议不同，不给预设下拉，保持输入框。
const WHISPER_COMPAT_ASR_PROVIDERS: AsrPresetId[] = ['whisper', 'groq', 'siliconflow', 'openrouter', 'openai-compatible'];

/** 模型预设下拉里的「自定义模型…」哨兵值：选中即切回输入框手输。 */
const CUSTOM_MODEL_OPTION_VALUE = '__custom_model__';

type ProvidersSectionKind = 'all' | 'llm' | 'asr';

interface ProvidersSectionProps {
  kind?: ProvidersSectionKind;
}

export function ProvidersSection({ kind = 'all' }: ProvidersSectionProps = {}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const mobile = useMobileLayout();
  // `*Provider` 立即跟随 <select> 改动（受控组件必须实时反映用户输入）；
  // `committed*Provider` 才决定 CredentialField 的 key，仅在后端 active
  // 切换 + 默认值写完后再 commit。两者拆开是为了同时满足：
  //   - <select> 立刻显示用户的选择（issue #220 P2：codex 指出受控选不应等 await）
  //   - CredentialField 不要在后端 active 切完前 remount（issue #219：避免读到旧 entry）
  // `*SwitchSeq` 是 stale-write 守卫：用户 100ms 内连点两次时，先发的请求晚到不
  // 会覆盖后发的 commit。
  const [llmProvider, setLlmProvider] = useState<LlmPresetId>('ark');
  const [asrProvider, setAsrProvider] = useState<AsrPresetId>('volcengine');
  const [committedLlmProvider, setCommittedLlmProvider] = useState<LlmPresetId>('ark');
  const [committedAsrProvider, setCommittedAsrProvider] = useState<AsrPresetId>('volcengine');
  const llmSwitchSeqRef = useRef(0);
  const asrSwitchSeqRef = useRef(0);
  const [llmModelRevision, setLlmModelRevision] = useState(0);
  const [asrModelRevision, setAsrModelRevision] = useState(0);
  const os = detectOS();
  const unifiedBailian = committedAsrProvider === 'bailian';
  const [bailianModel, setBailianModel] = useState('');
  const [volcengineAuthMode, setVolcengineAuthMode] = useState<'app_id_token' | 'api_key'>('app_id_token');

  useEffect(() => {
    if (committedAsrProvider === 'volcengine') {
      readCredential('volcengine.auth_mode', 'volcengine')
        .then(v => {
          if (v === 'api_key') setVolcengineAuthMode('api_key');
          else setVolcengineAuthMode('app_id_token');
        })
        .catch(() => setVolcengineAuthMode('app_id_token'));
    }
  }, [committedAsrProvider]);

  useEffect(() => {
    if (committedAsrProvider !== 'bailian') setBailianModel('');
  }, [committedAsrProvider]);
  // 本地引擎（qwen3 / sherpa / foundry）的已下载模型直接作为 ASR 供应商下拉的
  // 可选项：模型 ID 就是选项，选中即使用该模型（不用先选引擎再选模型）。
  // 一次拉全三个引擎；并监听下载进度事件，在本地模型下载完成后自动刷新列表。
  const [localModelOptions, setLocalModelOptions] = useState<
    { engine: 'qwen3' | 'sherpa' | 'foundry'; id: string; name: string; isDownloaded: boolean }[]
  >([]);
  // 同 provider 内切换本地模型的乐观值：下拉立即显示用户点的模型，不等后端
  // set_settings + prefs:changed 事件回来（回来前的几帧会闪回旧模型 = 闪烁）。
  const [localAsrModelDraft, setLocalAsrModelDraft] = useState<string | null>(null);
  useEffect(() => {
    // 供应商切换后旧引擎的 draft 不再适用，清掉让 asrValue 回退 prefs。
    setLocalAsrModelDraft(null);
  }, [committedAsrProvider]);
  useEffect(() => {
    let cancelled = false;
    // 平台分支拉取：sherpa/foundry 的 catalog 命令只在 Windows 注册，macOS 上
    // invoke 未注册命令会 reject——Promise.all 拉三个会把 qwen3 的结果也一起
    // 吞掉（下拉永远只剩引擎级入口）。按平台只拉本平台存在的引擎。
    const fetchAll = async () => {
      try {
        const qwen3 = await listLocalAsrModels();
        const extra =
          os === 'win'
            ? await Promise.all([
                getSherpaOnnxAsrCatalog(),
                getFoundryLocalAsrCatalog(),
              ])
            : null;
        if (cancelled) return;
        const next = [
          ...qwen3.map(m => ({ engine: 'qwen3' as const, id: m.id, name: m.id, isDownloaded: m.isDownloaded })),
          ...(extra?.[0] ?? []).map(c => ({ engine: 'sherpa' as const, id: c.alias, name: c.displayName || c.alias, isDownloaded: c.cached })),
          ...(extra?.[1] ?? []).map(c => ({ engine: 'foundry' as const, id: c.alias, name: c.displayName || c.alias, isDownloaded: c.cached })),
        ];
        // 浅比较：数据没变就不 setState，避免 3s 轮询让下拉每轮重渲染（闪烁）。
        setLocalModelOptions(prev =>
          prev.length === next.length &&
          prev.every((m, i) =>
            m.engine === next[i].engine &&
            m.id === next[i].id &&
            m.name === next[i].name &&
            m.isDownloaded === next[i].isDownloaded,
          )
            ? prev
            : next,
        );
      } catch {
        if (!cancelled) setLocalModelOptions([]);
      }
    };
    void fetchAll();
    // 3s 轮询磁盘状态：模型被外部删除（或下载完成后）下拉选项自动跟随，
    // 用户不需要重开设置页。本地 fs 检查很轻，无感。
    const pollTimer = window.setInterval(() => {
      void fetchAll();
    }, 3000);
    // 下载完成事件驱动刷新：本页下方「本地模型」看板下载完模型后，下拉立刻出现新选项。
    let unlistenQ: (() => void) | undefined;
    let unlistenS: (() => void) | undefined;
    void import('@tauri-apps/api/event').then(({ listen }) => {
      void listen<{ phase: string }>('local-asr-download-progress', (e) => {
        if (e.payload.phase === 'finished') void fetchAll();
      }).then(fn => { if (cancelled) fn(); else unlistenQ = fn; }).catch(() => {});
      void listen<{ phase: string }>('sherpa-onnx-asr-download-progress', (e) => {
        if (e.payload.phase === 'finished') void fetchAll();
      }).then(fn => { if (cancelled) fn(); else unlistenS = fn; }).catch(() => {});
    }).catch(() => {});
    return () => {
      cancelled = true;
      window.clearInterval(pollTimer);
      unlistenQ?.();
      unlistenS?.();
    };
  }, []);

  // 本地引擎（qwen3 / foundry / sherpa）直接作为常规选项放进主下拉（按平台 gating），
  // 选项名标注「本地」——选了本地模型供应商，ASR 就用本地模型（与 Apple 语音同理），
  // 不再需要单独的启用开关。模型下载与管理在「服务 → 本地模型」的看板里。
  const visibleAsrPresets = ASR_PRESETS.filter(
    p => (p.id !== 'foundry-local-whisper' || os === 'win')
      && (p.id !== 'sherpa-onnx-local' || os === 'win')
      && (p.id !== 'local-qwen3' || os === 'mac')
      && (p.id !== 'apple-speech' || os === 'mac')
      // 百炼三协议收成一个「阿里云百炼」入口(id=bailian)+ 模型下拉。qwen3 / fun-asr-flash
      // 两个旧 id 作隐藏别名:新用户下拉里看不到,只有已经停在该 id 上的老用户仍显示,
      // 保证其配置不被打断(见 coordinator::resolve_effective_asr_provider 的向后兼容)。
      && (p.id !== 'bailian-qwen3-realtime' || asrProvider === 'bailian-qwen3-realtime')
      && (p.id !== 'bailian-fun-asr-flash' || asrProvider === 'bailian-fun-asr-flash'),
  );

  useEffect(() => {
    if (!prefs) return;
    const knownLlm = LLM_PRESETS.find(x => x.id === prefs.activeLlmProvider);
    const llmId = knownLlm ? knownLlm.id : 'custom';
    setLlmProvider(llmId);
    setCommittedLlmProvider(llmId);
    // ASR 在 ALL ASR_PRESETS 里查（不是 visibleAsrPresets）——本地选项虽然
    // 从下拉里藏起来了，但若用户曾在「高级」里启用过 local-qwen3，主 Card
    // 仍要识别出 active 是本地，并切到「正在使用本地 ASR」的 notice 渲染。
    const knownAsr = ASR_PRESETS.find(x => x.id === prefs.activeAsrProvider);
    const asrId = knownAsr ? knownAsr.id : 'volcengine';
    setAsrProvider(asrId);
    setCommittedAsrProvider(asrId);
  }, [prefs, os]);

  // issue #219 / #220 P2：
  //   1. 立刻 setLlmProvider —— 受控 <select> 必须反映用户最新选择。
  //   2. 用 seq 守卫每个 await：用户连点两次时旧请求晚到也不会盖掉新选择。
  //   3. 仅 setCommittedLlmProvider 之后 CredentialField 才 remount 读新 entry，
  //      此时后端 root.active.llm 已经是 id，lookup_account 落到正确 entry。
  //   4. endpoint/model 默认值仅在该 provider entry 该字段为空时才填，不覆盖用户自定义。
  const onLlmProviderChange = async (id: LlmPresetId) => {
    setLlmProvider(id);
    const seq = ++llmSwitchSeqRef.current;
    emitSaved('saving', t('common.saving'));
    // 后端 active.llm 是否已切到 id —— 决定失败时下拉框该回滚到哪。
    let backendSwitched = false;
    try {
      await setActiveLlmProvider(id);
      backendSwitched = true;
      if (seq !== llmSwitchSeqRef.current) return;
      if (prefs) {
        const next = { ...prefs, activeLlmProvider: id };
        await updatePrefs(next);
        if (seq !== llmSwitchSeqRef.current) return;
      }
      const preset = LLM_PRESETS.find(p => p.id === id);
      // 修 bug：所有 LLM provider 共用 `ark.endpoint` / `ark.model_id` 一对凭据槽
      // （persistence.rs 没做 per-provider 隔离）。旧逻辑只在槽空时填默认值，
      // 老用户切换 preset 时槽里早有旧值——dropdown 看着切了，polish 实际还是
      // 打老 endpoint。改成：切到任何非 custom 预设都强制覆盖 endpoint 与 model
      // 到该预设的默认值，让"切换"真切到位。custom 预设没有默认值，跳过。
      if (preset && preset.id !== 'custom') {
        if (preset.baseUrl) {
          await setCredential('ark.endpoint', preset.baseUrl);
          if (seq !== llmSwitchSeqRef.current) return;
        }
        if (preset.modelPlaceholder) {
          await setCredential('ark.model_id', preset.modelPlaceholder);
          if (seq !== llmSwitchSeqRef.current) return;
        }
      }
      setCommittedLlmProvider(id);
      emitSaved('saved', t('common.saved'));
    } catch (err) {
      // seq 守卫：只有当前 call 还是最新时才翻 failed + 回滚下拉框；旧 call 早被
      // newer call 的 emitSaved('saving') 覆盖，不要插手。
      if (seq === llmSwitchSeqRef.current) {
        emitSaved('failed', t('common.operationFailed'));
        // 仅当后端切换本身没成（active.llm 仍是旧的）才回滚下拉框 —— 回到 committed
        // 与后端一致。若后端已切到 id、只是后续 prefs / 凭据写入失败，回滚反而让下拉
        // 显示旧、后端是新；此时保持下拉在 id 与后端一致更不误导。
        if (!backendSwitched) {
          setLlmProvider(committedLlmProvider);
        }
      }
      // 不再 rethrow：本 handler 作为 SelectLite onChange 是即发即忘调用，
      // rethrow 会变成未处理的 promise rejection。错误已 emitSaved + 记日志。
      console.error('[settings] switch LLM provider failed', err);
    }
  };

  const onLlmThinkingToggle = (enabled: boolean) => {
    if (!prefs) return;
    void updatePrefs(current => ({ ...current, llmThinkingEnabled: enabled })).catch(error => {
      console.error('[settings] failed to update LLM thinking mode', error);
      emitSaved('failed', t('common.operationFailed'));
    });
  };

  const onAsrProviderChange = async (id: AsrPresetId, modelId?: string) => {
    setAsrProvider(id);
    // 轻量路径：供应商没变、只是换本地模型 → 不重跑 set_active_provider /
    // 凭据回填整套流程（那些会触发 prefs:changed 全量重渲染 + 下拉闪回旧值），
    // 只写模型命令 + prefs 字段。draft 让下拉立即显示用户点的模型。
    if (id === committedAsrProvider && modelId && isLocalAsrPreset(id)) {
      setLocalAsrModelDraft(modelId);
      try {
        if (id === 'local-qwen3') {
          await setLocalAsrActiveModel(modelId);
        } else if (id === 'sherpa-onnx-local') {
          await setSherpaOnnxAsrModel(modelId);
        } else if (id === 'foundry-local-whisper') {
          await setFoundryLocalAsrModel(modelId);
        }
        if (prefs) {
          const next = { ...prefs, activeAsrProvider: id };
          if (id === 'local-qwen3') next.localAsrActiveModel = modelId;
          else if (id === 'sherpa-onnx-local') next.sherpaOnnxModel = modelId;
          else if (id === 'foundry-local-whisper') next.foundryLocalAsrModel = modelId;
          await updatePrefs(next);
        }
        emitSaved('saved', t('common.saved'));
      } catch (err) {
        // 写入失败回滚 draft，让下拉回到 prefs 里的真实值。
        setLocalAsrModelDraft(null);
        emitSaved('failed', t('common.operationFailed'));
        console.error('[settings] switch local ASR model failed', err);
      }
      return;
    }
    const seq = ++asrSwitchSeqRef.current;
    emitSaved('saving', t('common.saving'));
    let backendSwitched = false;
    try {
      await setActiveAsrProvider(id);
      backendSwitched = true;
      if (seq !== asrSwitchSeqRef.current) return;
      // 模型 ID 直选：供应商切到本地引擎后，把 active model 一并写进后端。
      if (modelId) {
        if (id === 'local-qwen3') {
          await setLocalAsrActiveModel(modelId);
        } else if (id === 'sherpa-onnx-local') {
          await setSherpaOnnxAsrModel(modelId);
        } else if (id === 'foundry-local-whisper') {
          await setFoundryLocalAsrModel(modelId);
        }
        if (seq !== asrSwitchSeqRef.current) return;
      }
      if (prefs) {
        const next = { ...prefs, activeAsrProvider: id };
        if (modelId) {
          if (id === 'local-qwen3') next.localAsrActiveModel = modelId;
          else if (id === 'sherpa-onnx-local') next.sherpaOnnxModel = modelId;
          else if (id === 'foundry-local-whisper') next.foundryLocalAsrModel = modelId;
        }
        await updatePrefs(next);
        if (seq !== asrSwitchSeqRef.current) return;
      }
      // 凭据按 provider 隔离。切换回来时优先保留该 provider 已保存的自定义值，
      // 仅在当前 entry 为空时写入 preset 默认值。
      const preset = ASR_PRESETS.find(p => p.id === id);
      const [storedEndpoint, storedModel] = await Promise.all([
        readCredential('asr.endpoint', id),
        readCredential('asr.model', id),
      ]);
      if (seq !== asrSwitchSeqRef.current) return;
      if (preset?.baseUrl && !storedEndpoint?.trim()) {
        await setCredential('asr.endpoint', preset.baseUrl, id);
        if (seq !== asrSwitchSeqRef.current) return;
      }
      if (preset?.model && !storedModel?.trim()) {
        await setCredential('asr.model', preset.model, id);
        if (seq !== asrSwitchSeqRef.current) return;
      }
      setCommittedAsrProvider(id);
      emitSaved('saved', t('common.saved'));
    } catch (err) {
      // seq 守卫 + 回滚 + 不 rethrow，同 onLlmProviderChange。
      if (seq === asrSwitchSeqRef.current) {
        emitSaved('failed', t('common.operationFailed'));
        // 同 onLlmProviderChange：仅后端没切成时才回滚下拉框，与后端保持一致。
        if (!backendSwitched) {
          setAsrProvider(committedAsrProvider);
        }
      }
      console.error('[settings] switch ASR provider failed', err);
    }
  };

  // preset 决定 placeholder 与 default —— 必须跟着 committed*Provider 走，
  // 否则受控 <select> 立刻切到新厂商，但凭据字段还在显示旧 entry，placeholder
  // 会先于实际数据切换、视觉上对不上。
  const preset = LLM_PRESETS.find(p => p.id === committedLlmProvider) ?? LLM_PRESETS[LLM_PRESETS.length - 1];
  const codexOAuthSelected = committedLlmProvider === 'codex_oauth';
  const asrPreset = visibleAsrPresets.find(p => p.id === committedAsrProvider);
  const showLlm = kind === 'all' || kind === 'llm';
  const showAsr = kind === 'all' || kind === 'asr';
  return (
    <>
      {kind === 'all' && (
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}>
        {t('settings.providers.credentialStorageNotice')}
      </div>
      )}
      {showLlm && (
      <Card>
        <div style={{ marginBottom: 10 }}>
          <SectionTitle>{t('settings.providers.llmTitle')}</SectionTitle>
        </div>
        {/* desc 已去掉——'选择后将自动填入 Base URL 默认值' 在 180px label 列必换行成两行，
            视觉上 label 区出现"字体单独占一行"。下拉自身已经表达了"切换"含义，desc 冗余。 */}
        <SettingRow label={t('settings.providers.providerLabel')}>
          <SelectLite
            value={llmProvider}
            onChange={next => onLlmProviderChange(next as LlmPresetId)}
            options={LLM_PRESETS.map(p => ({
              value: p.id,
              label: t(`settings.providers.presets.${p.nameKey}`),
            }))}
            ariaLabel={t('settings.providers.providerLabel')}
            style={{ width: mobile ? '100%' : 200, maxWidth: '100%', minWidth: 0 }}
          />
        </SettingRow>
        {codexOAuthSelected ? (
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, margin: '2px 0 10px' }}>
            {t('settings.providers.codexOAuthNotice')}
          </div>
        ) : (
          <>
            <CredentialField key={`${committedLlmProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="ark.api_key" mono mask />
            <CredentialField key={`${committedLlmProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="ark.endpoint"
              placeholder={preset.baseUrl || 'https://your-endpoint/v1'} />
            {committedLlmProvider === 'custom' && (
              <>
                <CredentialField
                  key={`${committedLlmProvider}:temperature`}
                  label={t('settings.providers.temperatureLabel')}
                  account="ark.temperature"
                  placeholder={t('settings.providers.temperaturePlaceholder')}
                  mono
                />
                <CredentialField
                  key={`${committedLlmProvider}:extra_headers`}
                  label={t('settings.providers.extraHeadersLabel')}
                  account="ark.extra_headers"
                  placeholder={t('settings.providers.extraHeadersPlaceholder')}
                  mono
                  mask
                />
              </>
            )}
          </>
        )}
        <CredentialField key={`${committedLlmProvider}:model:${llmModelRevision}`} label={t('settings.providers.modelLabel')} account="ark.model_id"
          placeholder={preset.modelPlaceholder || 'model-name'} mono
          trailing={(
            <LlmThinkingToggle
              enabled={prefs?.llmThinkingEnabled ?? false}
              onToggle={onLlmThinkingToggle}
            />
          )}
        />
        <ProviderTools key={committedLlmProvider} kind="llm" modelAccount="ark.model_id" onModelSelected={() => setLlmModelRevision(v => v + 1)} />
      </Card>
      )}

      {showAsr && (
      <Card>
        <div style={{ marginBottom: 10 }}>
          <SectionTitle>{t('settings.providers.asrTitle')}</SectionTitle>
        </div>
        {/* 下拉只放云端选项；本地引擎激活时锁住 + 在下方放一行"ASR 提供商已被接管"提示，
            未激活时不显示提示。 */}
        <SettingRow label={t('settings.providers.providerLabel')}>
          {(() => {
            // 本地引擎的已下载模型直接作为下拉选项（value = "引擎:模型ID"）：
            // 选了哪个模型 ID，就用哪个模型——不用先选引擎再选模型。
            // 引擎 → 模型数据源映射。
            const LOCAL_ENGINE_OF: Record<string, 'qwen3' | 'sherpa' | 'foundry'> = {
              'local-qwen3': 'qwen3',
              'sherpa-onnx-local': 'sherpa',
              'foundry-local-whisper': 'foundry',
            };
            const localPresets = visibleAsrPresets.filter(p => isLocalAsrPreset(p.id));
            const localOptions = localPresets.flatMap(p => {
              const downloaded = localModelOptions.filter(
                m => m.engine === LOCAL_ENGINE_OF[p.id] && m.isDownloaded,
              );
              if (downloaded.length === 0) {
                // 还没下载模型：保留引擎入口，选它之后去「本地模型」看板下载。
                return [{
                  value: p.id,
                  label: `${t(`settings.providers.presets.${p.nameKey}`)}（${t('settings.providers.localTag')}）`,
                }];
              }
              return downloaded.map(m => ({
                value: `${p.id}:${m.id}`,
                label: `${m.name}（${t('settings.providers.localTag')}）`,
              }));
            });
            // 受控 value：本地引擎激活且 active 模型已下载时显示 "引擎:模型ID"。
            // draft（用户刚点的模型）优先于 prefs——同 provider 换模型时后端
            // 还没回写完成，直接读 prefs 会闪回旧模型。
            const activeModelId = committedAsrProvider === 'local-qwen3'
              ? prefs?.localAsrActiveModel
              : committedAsrProvider === 'sherpa-onnx-local'
                ? prefs?.sherpaOnnxModel
                : committedAsrProvider === 'foundry-local-whisper'
                  ? prefs?.foundryLocalAsrModel
                  : undefined;
            const resolvedModelId =
              localAsrModelDraft &&
              localModelOptions.some(m => m.id === localAsrModelDraft && m.isDownloaded)
                ? localAsrModelDraft
                : activeModelId;
            const asrValue =
              isLocalAsrPreset(committedAsrProvider) &&
              resolvedModelId &&
              localModelOptions.some(m => m.id === resolvedModelId && m.isDownloaded)
                ? `${committedAsrProvider}:${resolvedModelId}`
                : asrProvider;
            // 平台不匹配的旧配置（如 Windows 上仍激活 local-qwen3）：补一个选项兜底。
            const hiddenLocalActive: AsrPresetId | null =
              !visibleAsrPresets.some(p => p.id === committedAsrProvider)
                ? committedAsrProvider
                : null;
            // 本地引擎激活但 active 模型不在已下载列表（模型被删）时，value 无匹配
            // 选项——补引擎兜底项让下拉有显示、可切走。hiddenLocalActive 已兜底时不重复加。
            const unmatchedLocalPreset =
              isLocalAsrPreset(asrValue) && !hiddenLocalActive
                ? ASR_PRESETS.find(p => p.id === asrValue)
                : undefined;
            const hiddenLocalNameKey = hiddenLocalActive === 'local-qwen3'
              ? 'asrLocalQwen3'
              : hiddenLocalActive === 'foundry-local-whisper'
                ? 'asrFoundryLocalWhisper'
                : hiddenLocalActive === 'sherpa-onnx-local'
                  ? 'asrSherpaOnnxLocal'
                  : hiddenLocalActive === 'apple-speech'
                    ? 'asrAppleSpeech'
                    : null;
            return (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6, alignItems: mobile ? 'stretch' : 'flex-start', minWidth: 0, width: '100%', maxWidth: '100%' }}>
                <SelectLite
                  value={asrValue}
                  onChange={(next) => {
                    const sep = next.indexOf(':');
                    if (sep > 0 && isLocalAsrPreset(next.slice(0, sep))) {
                      void onAsrProviderChange(next.slice(0, sep) as AsrPresetId, next.slice(sep + 1));
                    } else {
                      void onAsrProviderChange(next as AsrPresetId);
                    }
                  }}
                  options={[
                    ...visibleAsrPresets.filter(p => !isLocalAsrPreset(p.id)).map(p => ({
                      value: p.id,
                      label: t(`settings.providers.presets.${p.nameKey}`),
                    })),
                    ...localOptions,
                    ...(unmatchedLocalPreset && !localOptions.some(o => o.value === asrValue)
                      ? [{
                          value: asrValue,
                          label: `${t(`settings.providers.presets.${unmatchedLocalPreset.nameKey}`)}（${t('settings.providers.localTag')}）`,
                        }]
                      : []),
                    ...(hiddenLocalActive && hiddenLocalNameKey
                      ? [{
                          value: hiddenLocalActive,
                          label: t(`settings.providers.presets.${hiddenLocalNameKey}`),
                        }]
                      : []),
                  ]}
                  ariaLabel={t('settings.providers.providerLabel')}
                  style={{ width: mobile ? '100%' : 200, maxWidth: '100%', minWidth: 0 }}
                />
                {hiddenLocalActive && (
                  <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
                    {t('settings.providers.asrProviderTakenOver')}
                  </div>
                )}
              </div>
            );
          })()}
        </SettingRow>
        {/* 供应商切换时 ASR 板块高度 / 内容会变：keyed 淡入动画平滑过渡（ol-tab-fade）。 */}
        <div key={committedAsrProvider} style={{ animation: 'ol-tab-fade 0.22s var(--ol-motion-soft)' }}>
        {committedAsrProvider === 'volcengine' ? (
          <>
            <SettingRow label={t('settings.providers.volcengineAuthModeLabel')}>
              <SelectLite
                value={volcengineAuthMode}
                onChange={async (v) => {
                  const mode = v as 'app_id_token' | 'api_key';
                  const prev = volcengineAuthMode;
                  setVolcengineAuthMode(mode);
                  try {
                    await setCredential('volcengine.auth_mode', mode, committedAsrProvider);
                  } catch (error) {
                    // 写入失败必须回滚 UI 并提示：否则模式看着已切换、重启后却静默回退，
                    // 配合独立 API Key 槽会造成「Key 存在但模式不对」的混乱。
                    console.error('[settings] failed to save volcengine auth mode', error);
                    setVolcengineAuthMode(prev);
                    emitSaved('failed', t('common.operationFailed'));
                  }
                }}
                options={[
                  { value: 'app_id_token', label: t('settings.providers.volcengineAuthModeAppIdToken') },
                  { value: 'api_key', label: t('settings.providers.volcengineAuthModeApiKey') },
                ]}
                ariaLabel={t('settings.providers.volcengineAuthModeLabel')}
                style={{ width: mobile ? '100%' : 260, maxWidth: '100%', minWidth: 0 }}
              />
            </SettingRow>
            {/* 两种模式使用各自独立的凭据槽位：旧版 Access Token（volcengine.access_key）
                与方舟 API Key（volcengine.api_key）互不预填，切换模式不会残留混淆。 */}
            {volcengineAuthMode === 'app_id_token' ? (
              <>
                <CredentialField
                  key={`${committedAsrProvider}:app_key`}
                  label={t('settings.providers.volcengineAppKeyLabel')}
                  account="volcengine.app_key"
                  provider={committedAsrProvider}
                  mono
                  mask
                />
                <CredentialField
                  key={`${committedAsrProvider}:access_key`}
                  label={t('settings.providers.volcengineAccessKeyLabel')}
                  account="volcengine.access_key"
                  provider={committedAsrProvider}
                  mono
                  mask
                />
              </>
            ) : (
              <CredentialField
                key={`${committedAsrProvider}:api_key`}
                label={t('settings.providers.volcengineApiKeyLabel')}
                account="volcengine.api_key"
                provider={committedAsrProvider}
                mono
                mask
              />
            )}
            <CredentialField
              key={`${committedAsrProvider}:resource_id`}
              label={t('settings.providers.volcengineResourceIdLabel')}
              account="volcengine.resource_id"
              provider={committedAsrProvider}
              mono
              placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {volcengineAuthMode === 'api_key'
                ? t('settings.providers.volcengineApiKeyNote')
                : t('settings.providers.volcengineMappingNote')}
            </div>
          </>
        ) : committedAsrProvider === 'iflytek' ? (
          <>
            <CredentialField
              key={`${committedAsrProvider}:app_id`}
              label={t('settings.providers.xfyunAppIdLabel')}
              account="xfyun.app_id"
              provider={committedAsrProvider}
              mono
            />
            <CredentialField
              key={`${committedAsrProvider}:api_key`}
              label={t('settings.providers.xfyunApiKeyLabel')}
              account="xfyun.api_key"
              provider={committedAsrProvider}
              mono
              mask
            />
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {t('settings.providers.xfyunNote')}
            </div>
          </>
        ) : committedAsrProvider === 'local-qwen3' || committedAsrProvider === 'foundry-local-whisper' || committedAsrProvider === 'sherpa-onnx-local' || committedAsrProvider === 'apple-speech' ? (
          // 本地引擎激活：模型选择已并进上方供应商下拉（模型 ID 直选），这里只留提示。
          // Apple 语音零模型选择。
          committedAsrProvider === 'apple-speech' ? (
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {t('settings.providers.appleSpeechLocalNote')}
            </div>
          ) : (
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {t('settings.providers.localEngineNote')}
            </div>
          )
        ) : (
          <>
            <CredentialField key={`${committedAsrProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="asr.api_key" provider={committedAsrProvider} mono mask />
            {/* 统一百炼保留 endpoint 供用户选择区域或工作空间域名；后端按模型转换协议与路径。 */}
            <CredentialField key={`${committedAsrProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="asr.endpoint"
              provider={committedAsrProvider}
              placeholder={asrPreset?.baseUrl || 'https://api.openai.com/v1'}
              defaultValue={asrPreset?.baseUrl || undefined} />
            <CredentialField key={`${committedAsrProvider}:model:${asrModelRevision}`} label={t('settings.providers.modelLabel')} account="asr.model"
              provider={committedAsrProvider}
              placeholder={unifiedBailian ? 'fun-asr-realtime' : (asrPreset?.model || 'whisper-1')}
              onValueChange={unifiedBailian ? setBailianModel : undefined}
              options={unifiedBailian
                ? BAILIAN_ASR_MODELS.map(m => ({ value: m, label: m }))
                : WHISPER_COMPAT_ASR_PROVIDERS.includes(committedAsrProvider)
                  ? OPENAI_COMPAT_ASR_MODELS.map(m => ({ value: m, label: m }))
                  : undefined} />
            {unifiedBailian && (
              <BailianProtocolHint key={`${committedAsrProvider}:proto:${asrModelRevision}`} currentModel={bailianModel} />
            )}
            {unifiedBailian && bailianModelSupportsVocabulary(bailianModel) && (
              <>
                <CredentialField
                  key={`${committedAsrProvider}:vocabulary_id`}
                  label={t('settings.providers.bailianVocabularyIdLabel')}
                  account="asr.vocabulary_id"
                  provider={committedAsrProvider}
                  mono
                  placeholder="vocab-..."
                />
                <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
                  {t('settings.providers.bailianVocabularyIdNote')}
                </div>
              </>
            )}
            {committedAsrProvider === 'elevenlabs' && (
              <div role="note" style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
                {t('settings.providers.elevenLabsUploadNotice')}
              </div>
            )}
            {committedAsrProvider === 'zenmux' && (
              <div role="note" style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
                {t('settings.providers.zenmuxVocabularyNote')}
              </div>
            )}
            {/* 统一百炼「拉取模型」只写 model，不覆盖用户选择的区域或工作空间 endpoint。 */}
            <ProviderTools kind="asr" modelAccount="asr.model" provider={committedAsrProvider} onModelSelected={() => setAsrModelRevision(v => v + 1)} />
            {(committedAsrProvider === 'openai-compatible' || committedAsrProvider === 'zenmux') && (
              <AsrAdvancedOptions provider={committedAsrProvider} />
            )}
          </>
        )}
        </div>
      </Card>
      )}
    </>
  );
}

// ASR 高级选项：openai-compatible 与 zenmux 两个预设显示。
// openai-compatible 暴露 verbose_json / 分片时长（其余命名厂商保持硬编码行为）；
// zenmux 暴露 enable_itn（数字归一化）开关，verbose_json / 分片对其无意义。
function AsrAdvancedOptions({ provider }: { provider: string }) {
  const { t } = useTranslation();
  const [verboseJson, setVerboseJson] = useState(false);
  const [chunkDraft, setChunkDraft] = useState('');
  const [enableItn, setEnableItn] = useState(true);
  const [status, setStatus] = useState<'idle' | 'saving' | 'error'>('idle');
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;
    setStatus('idle');
    setError('');
    void (async () => {
      try {
        const raw = await readCredential('asr.advanced_config', provider);
        if (cancelled) return;
        const config = parseAdvancedAsrConfig(raw);
        setVerboseJson(config.verboseJson);
        setChunkDraft(config.chunkDurationMs ? String(config.chunkDurationMs) : '');
        setEnableItn(config.enableItn);
      } catch (err) {
        if (!cancelled) {
          setStatus('error');
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [provider]);

  const parseChunkDraft = (draft: string): number | null => {
    const value = Number(draft);
    if (draft.trim() === '' || !Number.isFinite(value) || value <= 0) return null;
    return Math.floor(value);
  };

  const save = async (partial: {
    verboseJson?: boolean
    chunkDurationMs?: number | null
    enableItn?: boolean
  }) => {
    setStatus('saving');
    setError('');
    const next: AdvancedAsrConfig = {
      verboseJson: partial.verboseJson ?? verboseJson,
      chunkDurationMs:
        partial.chunkDurationMs !== undefined
          ? partial.chunkDurationMs
          : parseChunkDraft(chunkDraft),
      enableItn: partial.enableItn ?? enableItn,
    };
    try {
      await setCredential('asr.advanced_config', serializeAdvancedAsrConfig(next), provider);
      setVerboseJson(next.verboseJson);
      setChunkDraft(next.chunkDurationMs ? String(next.chunkDurationMs) : '');
      setEnableItn(next.enableItn);
      setStatus('idle');
    } catch (err) {
      setStatus('error');
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <>
      <div
        role="note"
        style={{
          fontSize: 11.5,
          color: 'var(--ol-ink-4)',
          lineHeight: 1.6,
          margin: '2px 0 8px',
        }}
      >
        {t('settings.providers.asrAdvancedNote')}
      </div>
      {provider === 'zenmux' ? (
        <SettingRow
          label={t('settings.providers.asrAdvancedEnableItnLabel')}
          desc={t('settings.providers.asrAdvancedEnableItnHint')}
        >
          <Toggle on={enableItn} onToggle={(next) => void save({ enableItn: next })} />
        </SettingRow>
      ) : (
        <>
          <SettingRow
            label={t('settings.providers.asrAdvancedVerboseJsonLabel')}
            desc={t('settings.providers.asrAdvancedVerboseJsonHint')}
          >
            <Toggle on={verboseJson} onToggle={(next) => void save({ verboseJson: next })} />
          </SettingRow>
          <SettingRow
            label={t('settings.providers.asrAdvancedChunkLabel')}
            desc={t('settings.providers.asrAdvancedChunkHint')}
          >
            <input
              type="number"
              min={0}
              step={1000}
              value={chunkDraft}
              placeholder="0"
              disabled={status === 'saving'}
              onChange={(e) => setChunkDraft(e.target.value)}
              onBlur={() => void save({ chunkDurationMs: parseChunkDraft(chunkDraft) })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
              }}
              style={inputStyle}
            />
          </SettingRow>
        </>
      )}
      {status === 'error' && (
        <div style={{ fontSize: 11, color: 'var(--ol-warn)', lineHeight: 1.4 }}>
          {t('common.operationFailed')}: {error}
        </div>
      )}
    </>
  );
}

// 统一「阿里云百炼」下,按模型名判断走哪种协议(与后端
// coordinator::resolve_effective_asr_provider 保持一致):qwen3-asr-flash-realtime* 与
// fun-asr-realtime* 与 fun-asr-flash-8k-realtime* 都是实时模型；fun-asr-flash-2026-06-15
// 与 qwen-audio-3.0-asr-flash 是「录音文件·说完转写」（同步）。
function bailianModelProtocol(model: string): 'realtime' | 'sync' | 'async' {
  const m = model.trim();
  if (!m || m.includes('realtime')) return 'realtime';
  // qwen3-asr-flash-filetrans 仅接受公网 URL，暂不支持（后端 protocol_for_model
  // 显式拒绝），前端不再归为 async 提示。
  if (m === 'fun-asr'
    || m.startsWith('fun-asr-') && !m.startsWith('fun-asr-flash')
    || m.startsWith('paraformer')) return 'async';
  // 其余（fun-asr-flash-*、qwen3-asr-flash、qwen-audio-3.0-asr-flash）为同步录音模型。
  return 'sync';
}

// qwen-audio-3.0-asr-flash 官方支持热词，但批量协议尚未把该设置写入请求体；
// 在后端接入前不展示一个实际不生效的热词输入框。
function bailianModelSupportsVocabulary(model: string): boolean {
  const m = model.trim();
  return !m
    || m.startsWith('fun-asr-realtime')
    || m.startsWith('paraformer-realtime')
    || m.startsWith('sensevoice-realtime');
}

// 模型框下的一行协议提示,解决「三种模型看不出区别」——告诉用户当前模型是实时还是
// 录音文件、行为差异如何。随 asrModelRevision(拉取/选择模型时)与挂载时重读 asr.model。
function BailianProtocolHint({ currentModel }: { currentModel: string }) {
  const { t } = useTranslation();
  const [model, setModel] = useState('');

  useEffect(() => {
    let cancelled = false;
    readCredential('asr.model')
      .then(v => { if (!cancelled) setModel(v || 'fun-asr-realtime'); })
      .catch(() => { /* 读失败按默认实时提示 */ });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    setModel(currentModel || 'fun-asr-realtime');
  }, [currentModel]);

  const protocol = bailianModelProtocol(model);
  const hint = protocol === 'realtime'
    ? t('settings.providers.bailianModelRealtimeHint')
    : protocol === 'async'
      ? t('settings.providers.bailianModelAsyncFileHint')
      : t('settings.providers.bailianModelSyncFileHint');

  return (
    <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
      {hint}
    </div>
  );
}

type ProviderToolStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error';

function ProviderTools({ kind, modelAccount, provider, onModelSelected, showFetchModels = true }: { kind: 'llm' | 'asr'; modelAccount: string; provider?: string; onModelSelected: () => void; showFetchModels?: boolean }) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [status, setStatus] = useState<ProviderToolStatus>('idle');
  const [message, setMessage] = useState('');

  const setResult = (next: ProviderToolStatus, nextMessage: string) => {
    setStatus(next);
    setMessage(nextMessage);
  };

  const validate = async () => {
    setModels([]);
    setSelectedModel('');
    setResult('loading', t('settings.providers.validating'));
    try {
      const result = await validateProviderCredentials(kind);
      setResult(
        result.ok ? 'success' : 'error',
        t(result.ok ? 'settings.providers.validateSuccess' : 'settings.providers.validateFailed'),
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if ((kind === 'llm' && message === 'llmModelMissing') || (kind === 'asr' && message === 'asrModelMissing')) {
        setResult('empty', t('settings.providers.modelMissing'));
        return;
      }
      if (message === 'modelsEmpty') {
        setResult('empty', t('settings.providers.modelsEmpty'));
        return;
      }
      setResult('error', providerErrorMessage(error, t));
    }
  };

  const loadModels = async () => {
    setResult('loading', t('settings.providers.loadingModels'));
    try {
      const result = await listProviderModels(kind);
      setModels(result.models);
      if (result.models.length === 0) {
        setResult('empty', t('settings.providers.modelsEmpty'));
      } else {
        setSelectedModel('');
        setResult('success', t('settings.providers.modelsLoaded', { count: result.models.length }));
      }
    } catch (error) {
      setModels([]);
      setResult('error', providerErrorMessage(error, t));
    }
  };

  const applyModel = async (model: string) => {
    setResult('loading', t('common.saving'));
    try {
      await setCredential(modelAccount, model, provider);
      setSelectedModel(model);
      onModelSelected();
      setResult('success', t('settings.providers.modelSaved', { model }));
    } catch (error) {
      setResult('error', providerErrorMessage(error, t));
    }
  };

  return (
    <SettingRow label={t('settings.providers.toolsLabel')}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%', maxWidth: mobile ? '100%' : 420 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap', width: '100%' }}>
          <button onClick={validate} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.validate')}</button>
          {showFetchModels && (
            <button onClick={loadModels} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.fetchModels')}</button>
          )}
          {showFetchModels && models.length > 0 && (
            <SelectLite
              value={selectedModel}
              onChange={applyModel}
              disabled={status === 'loading'}
              options={models.map(model => ({ value: model, label: model }))}
              placeholder={t('settings.providers.selectModel')}
              ariaLabel={t('settings.providers.selectModel')}
              style={{ flex: mobile ? '1 1 100%' : '1 1 180px', maxWidth: mobile ? '100%' : 220, minWidth: 0 }}
            />
          )}
        </div>
        {message && (
          <span style={{ fontSize: 11, color: status === 'error' ? 'var(--ol-warn)' : status === 'empty' ? 'var(--ol-ink-4)' : 'var(--ol-ok)', lineHeight: 1.4 }}>
            {message}
          </span>
        )}
      </div>
    </SettingRow>
  );
}

function providerErrorMessage(error: unknown, t: ReturnType<typeof useTranslation>['t']): string {
  const message = error instanceof Error ? error.message : String(error);
  if (message.startsWith('providerHttpStatus:')) {
    return t('settings.providers.providerHttpStatus', { status: message.split(':')[1] || '?' });
  }
  if (message === 'endpointMustUseHttps') return t('settings.providers.endpointMustUseHttps');
  if (message === 'endpointInvalid') return t('settings.providers.endpointInvalid');
  if (message === 'bailianEndpointSchemeInvalid') return t('settings.providers.bailianEndpointSchemeInvalid');
  if (message === 'qwen3EndpointSchemeInvalid') return t('settings.providers.qwen3EndpointSchemeInvalid');
  if (message === 'providerResponseTooLarge') return t('settings.providers.responseTooLarge');
  if (message === 'asrInvalidJson') return t('settings.providers.asrInvalidJson');
  if (message === 'asrMissingTextField') return t('settings.providers.asrMissingTextField');
  if (message === 'providerNetworkError') return t('common.networkError');
  if (message === 'providerReadResponseFailed' || message === 'providerClientInitFailed') return t('common.operationFailed');
  if (message === 'providerRequestTimeout') return t('settings.providers.requestTimeout');
  if (message.includes('API Key')) return t('settings.providers.apiKeyMissing');
  if (message.includes('Endpoint')) return t('settings.providers.endpointMissing');
  if (message.includes('timeout') || message.includes('超时')) return t('settings.providers.requestTimeout');
  if (message.startsWith('task failed:') || message.startsWith('connection failed:') || message.startsWith('send failed:')) {
    return message;
  }
  return t('common.operationFailed');
}

type CredentialFieldStatus = 'idle' | 'saving' | 'saved' | 'readError' | 'saveError' | 'copied' | 'copyError';

interface CredentialFieldProps {
  label: string;
  account: string;
  provider?: string;
  placeholder?: string;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
  trailing?: ReactNode;
  onValueChange?: (value: string) => void;
  /** 提供则渲染为下拉（预设选择）代替输入框；当前值不在预设里时附加为自定义项。 */
  options?: SelectOption[];
}

function CredentialField({ label, account, provider, placeholder, mono, mask, defaultValue, trailing, onValueChange, options }: CredentialFieldProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<CredentialFieldStatus>('idle');
  // 预设下拉的「自定义模型…」逃生口：选中后切回输入框，保证后端支持的任意模型名都能手输。
  const [customModelMode, setCustomModelMode] = useState(false);
  const debounceRef = useRef<number | null>(null);
  const statusRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setDirty(false);
    setStatus('idle');
    setValue('');
    onValueChange?.('');
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    readCredential(account, provider)
      .then(v => {
        if (cancelled) return;
        setValue(v ?? '');
        onValueChange?.(v ?? '');
        setLoaded(true);
      })
      .catch(error => {
        if (cancelled) return;
        console.error('[settings] failed to read credential', account, error);
        onValueChange?.('');
        setLoaded(true);
        setStatus('readError');
      });
    return () => {
      cancelled = true;
    };
  }, [account, provider, onValueChange]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (debounceRef.current) clearTimeout(debounceRef.current);
      if (statusRef.current) clearTimeout(statusRef.current);
    };
  }, []);

  // 改造：除 readError（持续错误，留在输入旁标识字段不可用）外，所有 saving / saved /
  //   saveError / copied / copyError 一律发到右上角 SavedToast。原内联文案太挤、跟其它
  //   页面 toast 风格不统一。
  const showTemporaryStatus = (next: CredentialFieldStatus) => {
    if (next === 'saving') {
      emitSaved('saving', t('common.saving'));
    } else if (next === 'saved') {
      emitSaved('saved', t('common.saved'));
    } else if (next === 'saveError') {
      emitSaved('failed', t('common.operationFailed'));
    } else if (next === 'copied') {
      emitSaved('saved', t('common.copied'));
    } else if (next === 'copyError') {
      emitSaved('failed', t('common.operationFailed'));
    }
    setStatus(next);
    if (statusRef.current) clearTimeout(statusRef.current);
    statusRef.current = window.setTimeout(() => setStatus('idle'), 1600);
  };

  const save = async (v: string, force = false) => {
    if (!loaded || (!dirty && !force)) return;
    if (!mountedRef.current) return;
    setStatus('saving');
    emitSaved('saving', t('common.saving'));
    try {
      await setCredential(account, v, provider);
      if (!mountedRef.current) return;
      setDirty(false);
      showTemporaryStatus('saved');
    } catch (error) {
      if (!mountedRef.current) return;
      console.error('[settings] failed to save credential', account, error);
      showTemporaryStatus('saveError');
    }
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value;
    setValue(v);
    onValueChange?.(v);
    if (!loaded) return;
    setDirty(true);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => save(v, true), 300);
  };

  const onBlur = () => {
    if (!loaded || !dirty) return;
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    void save(value, true);
  };

  const fillDefault = async () => {
    if (!loaded || !defaultValue) return;
    setValue(defaultValue);
    onValueChange?.(defaultValue);
    setDirty(true);
    await save(defaultValue, true);
  };

  const onCopy = async () => {
    if (!value || !loaded) return;
    try {
      if (!navigator.clipboard?.writeText) {
        throw new Error('Clipboard API unavailable');
      }
      await navigator.clipboard.writeText(value);
      showTemporaryStatus('copied');
    } catch (error) {
      console.error('[settings] failed to copy credential', account, error);
      showTemporaryStatus('copyError');
    }
  };

  const inputType = mask && !revealed ? 'password' : 'text';
  const disabled = !loaded;
  const showInsecureAsrEndpointWarning = account === 'asr.endpoint'
    && value.trim().toLowerCase().startsWith('http://');

  return (
    <SettingRow label={label}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 5, width: '100%', maxWidth: mobile ? '100%' : 420 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', flexWrap: mobile ? 'wrap' : 'nowrap' }}>
          {options && !customModelMode ? (
            <SelectLite
              value={value}
              onChange={(v) => {
                // 「自定义模型…」逃生口：切回输入框手输任意模型名。
                if (v === CUSTOM_MODEL_OPTION_VALUE) {
                  setCustomModelMode(true);
                  return;
                }
                setValue(v);
                onValueChange?.(v);
                if (!loaded) return;
                setDirty(true);
                void save(v, true);
              }}
              options={[
                ...(value && !options.some(o => o.value === value) ? [{ value, label: value }] : []),
                ...options,
                { value: CUSTOM_MODEL_OPTION_VALUE, label: t('settings.providers.customModelLabel', 'Custom model…') },
              ]}
              placeholder={loaded ? placeholder : t('common.loading')}
              disabled={disabled}
              ariaLabel={label}
              style={{ flex: mobile ? '1 1 180px' : 1, minWidth: 0, maxWidth: '100%', fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
            />
          ) : (
            <input
              type={inputType}
              value={value}
              placeholder={loaded ? placeholder : t('common.loading')}
              onChange={handleChange}
              onBlur={onBlur}
              disabled={disabled}
              style={{ ...inputStyle, flex: mobile ? '1 1 180px' : 1, minWidth: 0, maxWidth: '100%', fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
            />
          )}
          {options && customModelMode && (
            <button
              onClick={() => setCustomModelMode(false)}
              title={t('settings.providers.presetListLabel', 'Back to presets')}
              style={iconBtnStyle}
              disabled={disabled}
            >
              <Icon name="chevDown" size={13} />
            </button>
          )}
          {defaultValue && !value && loaded && (
            <button onClick={fillDefault} title={t('settings.providers.fillDefault')} style={iconBtnStyle} disabled={!loaded}>
              <Icon name="check" size={13} />
            </button>
          )}
          {trailing}
          {mask && (
            <button
              onClick={() => setRevealed(r => !r)}
              title={revealed ? t('common.hide') : t('common.show')}
              style={iconBtnStyle}
              disabled={disabled}
            >
              <Icon name="eye" size={14} />
            </button>
          )}
          <button
            onClick={onCopy}
            title={t('common.copy')}
            style={iconBtnStyle}
            disabled={!value || disabled}
          >
            <Icon name="copy" size={14} />
          </button>
          {/* readError 是字段无法读取的持续错误，留在原位提示用户该字段不可用；
              其它瞬态状态（saving / saved / saveError / copied / copyError）都通过
              emitSaved 发到右上角统一 toast，不再内联占位。 */}
          {status === 'readError' && (
            <span
              style={{
                fontSize: 11,
                color: 'var(--ol-warn)',
                whiteSpace: 'nowrap',
              }}
            >
              {t('settings.providers.readFailed')}
            </span>
          )}
        </div>
        {showInsecureAsrEndpointWarning && (
          <span style={{ fontSize: 11, color: 'var(--ol-warn)', lineHeight: 1.45 }}>
            {t('settings.providers.endpointMustUseHttps')}
          </span>
        )}
      </div>
    </SettingRow>
  );
}

const miniBtnStyle: CSSProperties = {
  height: 32, padding: '0 12px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
  color: 'var(--ol-ink-2)', cursor: 'default', flexShrink: 0,
  fontSize: 12.5, fontWeight: 500, letterSpacing: '0.01em',
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.16s var(--ol-motion-quick)',
};

const iconBtnStyle: CSSProperties = {
  width: 32, height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  color: 'var(--ol-ink-3)', cursor: 'default', flexShrink: 0,
  transition: 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
};
