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
import { SelectLite } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle, inputStyle, type AsrPresetId } from './shared';

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

const LLM_PRESETS = [
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
    modelPlaceholder: 'gpt-5.3-codex-spark',
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
    id: 'custom',
    nameKey: 'custom',
    baseUrl: '',
    modelPlaceholder: '',
  },
] as const;

type LlmPresetId = typeof LLM_PRESETS[number]['id'];

const ASR_DEFAULT_RESOURCE_ID = 'volc.seedasr.sauc.duration';

// `volcengine` / `bailian` 走自建流式客户端；其余走 OpenAI 兼容
// `/audio/transcriptions`（`coordinator.rs::is_whisper_compatible_provider`）。
// 新增兼容厂商：
//   1. 在这里加一项 `{ id, nameKey, baseUrl, model }`；
//   2. 若走 Whisper 协议，`coordinator.rs::is_whisper_compatible_provider` 加同名 id；
//      若是专有协议，新增独立 ASR client 与 provider kind；
//   3. 在 i18n 的 `settings.providers.presets.<nameKey>` 加文案。
// `AsrPresetId` 定义在 settings/shared.tsx，LocalModelSection / ProvidersSection 共用同一份。
const ASR_PRESETS: ReadonlyArray<{ id: AsrPresetId; nameKey: string; baseUrl: string; model: string }> = [
  { id: 'volcengine',   nameKey: 'asrVolcengine',   baseUrl: '',                                              model: ''                              },
  { id: 'bailian',      nameKey: 'asrBailian',     baseUrl: 'wss://dashscope.aliyuncs.com/api-ws/v1/inference/', model: 'fun-asr-realtime'             },
  { id: 'siliconflow',  nameKey: 'asrSiliconflow',  baseUrl: 'https://api.siliconflow.cn/v1',                  model: 'FunAudioLLM/SenseVoiceSmall' },
  { id: 'zhipu',        nameKey: 'asrZhipu',        baseUrl: 'https://open.bigmodel.cn/api/paas/v4',           model: 'glm-asr-2512'                },
  { id: 'groq',         nameKey: 'asrGroq',         baseUrl: 'https://api.groq.com/openai/v1',                 model: 'whisper-large-v3-turbo'      },
  { id: 'whisper',      nameKey: 'asrWhisper',      baseUrl: 'https://api.openai.com/v1',                      model: 'whisper-1'                   },
  // OpenRouter 的 /audio/transcriptions 走 application/json + base64（issue #582），
  // 后端 coordinator.rs::whisper_request_format 对该 id 切换到 OpenRouterJson 编码。
  { id: 'openrouter',   nameKey: 'asrOpenrouter',   baseUrl: 'https://openrouter.ai/api/v1',                   model: 'openai/whisper-large-v3-turbo' },
  // 小米 MiMo ASR 按官方文档走 /chat/completions + input_audio，不是
  // Whisper /audio/transcriptions；后端由 asr/mimo.rs 专用 client 处理。
  { id: 'xiaomi-mimo-asr', nameKey: 'asrXiaomiMimo', baseUrl: 'https://api.xiaomimimo.com/v1',                  model: 'mimo-v2.5-asr'               },
  { id: 'foundry-local-whisper', nameKey: 'asrFoundryLocalWhisper', baseUrl: '',                              model: ''                              },
  // 本地引擎（Foundry / sherpa-onnx / Qwen3）：无 baseUrl/model 配置，
  // 模型在「高级 → 本地模型」里下载与切换。
  { id: 'sherpa-onnx-local',     nameKey: 'asrSherpaOnnxLocal',     baseUrl: '',                              model: ''                              },
  { id: 'local-qwen3',  nameKey: 'asrLocalQwen3',   baseUrl: '',                                              model: ''                              },
  // Apple 系统语音识别（macOS）：无 baseUrl/model、无下载、无凭据。
  { id: 'apple-speech', nameKey: 'asrAppleSpeech',  baseUrl: '',                                              model: ''                              },
];

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
  // 本地重引擎（qwen3 / sherpa / foundry）仍只在「高级 → 本地模型」里启用，
  // 防止新手在主下拉误开 CPU 推理。Apple 语音是系统自带、零凭据、轻量，
  // 在 macOS 上直接作为常规选项放进主下拉，方便随时选用 / 切走。
  const visibleAsrPresets = ASR_PRESETS.filter(
    p => p.id !== 'foundry-local-whisper'
      && p.id !== 'local-qwen3'
      && p.id !== 'sherpa-onnx-local'
      && (p.id !== 'apple-speech' || os === 'mac'),
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

  const onAsrProviderChange = async (id: AsrPresetId) => {
    setAsrProvider(id);
    const seq = ++asrSwitchSeqRef.current;
    emitSaved('saving', t('common.saving'));
    let backendSwitched = false;
    try {
      await setActiveAsrProvider(id);
      backendSwitched = true;
      if (seq !== asrSwitchSeqRef.current) return;
      if (prefs) {
        const next = { ...prefs, activeAsrProvider: id };
        await updatePrefs(next);
        if (seq !== asrSwitchSeqRef.current) return;
      }
      // asr.endpoint / asr.model 是所有 ASR 厂商共用的一对凭据槽（persistence.rs
      // 未做 per-provider 隔离）。若只在槽空时填默认值，老用户从 A 厂商切到 B 厂商
      // 时槽里仍是 A 的 endpoint/model —— dropdown 切了、实际还打 A 的地址。改成切到
      // 有默认值的预设就强制覆盖，让切换真切到位。volcengine 走另一套凭据、本地引擎
      // 无 baseUrl，都被 if 守卫天然跳过。与 onLlmProviderChange 同款修法。
      const preset = ASR_PRESETS.find(p => p.id === id);
      if (preset && preset.baseUrl) {
        await setCredential('asr.endpoint', preset.baseUrl);
        if (seq !== asrSwitchSeqRef.current) return;
      }
      if (preset && preset.model) {
        await setCredential('asr.model', preset.model);
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
            style={{ ...inputStyle, width: '100%', maxWidth: mobile ? '100%' : 200 }}
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
              <CredentialField
                key={`${committedLlmProvider}:extra_headers`}
                label={t('settings.providers.extraHeadersLabel')}
                account="ark.extra_headers"
                placeholder={t('settings.providers.extraHeadersPlaceholder')}
                mono
                mask
              />
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
            // 本地引擎激活时不再「接管 / 锁死」下拉——下拉始终可用，用户在本页就能直接
            // 切到其它供应商；切走后端 active 即自动停用本地引擎，不必再进「高级」手动关。
            // 重引擎（qwen3 / sherpa / foundry）当前激活但不在主下拉里时，补一个可选 option
            // 让 select 显示当前值并允许切走。Apple 语音在 macOS 已是常规可选项。
            const hiddenLocalActive: AsrPresetId | null =
              !visibleAsrPresets.some(p => p.id === committedAsrProvider)
                ? committedAsrProvider
                : null;
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
                  value={asrProvider}
                  onChange={next => onAsrProviderChange(next as AsrPresetId)}
                  options={[
                    ...visibleAsrPresets.map(p => ({
                      value: p.id,
                      label: t(`settings.providers.presets.${p.nameKey}`),
                    })),
                    ...(hiddenLocalActive && hiddenLocalNameKey
                      ? [{
                          value: hiddenLocalActive,
                          label: t(`settings.providers.presets.${hiddenLocalNameKey}`),
                        }]
                      : []),
                  ]}
                  ariaLabel={t('settings.providers.providerLabel')}
                  style={{ ...inputStyle, width: '100%', maxWidth: mobile ? '100%' : 200 }}
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
        {committedAsrProvider === 'volcengine' ? (
          <>
            <CredentialField
              key={`${committedAsrProvider}:app_key`}
              label={t('settings.providers.volcengineAppKeyLabel')}
              account="volcengine.app_key"
              mono
              mask
            />
            <CredentialField
              key={`${committedAsrProvider}:access_key`}
              label={t('settings.providers.volcengineAccessKeyLabel')}
              account="volcengine.access_key"
              mono
              mask
            />
            <CredentialField
              key={`${committedAsrProvider}:resource_id`}
              label={t('settings.providers.volcengineResourceIdLabel')}
              account="volcengine.resource_id"
              mono
              placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
            <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
              {t('settings.providers.volcengineMappingNote')}
            </div>
          </>
        ) : committedAsrProvider === 'local-qwen3' || committedAsrProvider === 'foundry-local-whisper' || committedAsrProvider === 'sherpa-onnx-local' || committedAsrProvider === 'apple-speech' ? (
          // 用户已经在用本地 ASR——dropdown 行的 asrProviderTakenOver 已经把
          // "在高级中切换或禁用"讲清楚了，body 不再重复。
          // 模型管理 UI 唯一入口在「高级 → 本地模型」里的 <LocalAsr embedded />。
          null
        ) : (
          <>
            <CredentialField key={`${committedAsrProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="asr.api_key" mono mask />
            <CredentialField key={`${committedAsrProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="asr.endpoint"
              placeholder={asrPreset?.baseUrl || 'https://api.openai.com/v1'}
              defaultValue={asrPreset?.baseUrl || undefined} />
            <CredentialField key={`${committedAsrProvider}:model:${asrModelRevision}`} label={t('settings.providers.modelLabel')} account="asr.model"
              placeholder={asrPreset?.model || 'whisper-1'} />
            {committedAsrProvider === 'bailian' && (
              <>
                <CredentialField
                  key={`${committedAsrProvider}:vocabulary_id`}
                  label={t('settings.providers.bailianVocabularyIdLabel')}
                  account="asr.vocabulary_id"
                  mono
                  placeholder="vocab-..."
                />
                <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
                  {t('settings.providers.bailianVocabularyIdNote')}
                </div>
              </>
            )}
            <ProviderTools kind="asr" modelAccount="asr.model" onModelSelected={() => setAsrModelRevision(v => v + 1)} />
          </>
        )}
      </Card>
      )}
    </>
  );
}

type ProviderToolStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error';

function ProviderTools({ kind, modelAccount, onModelSelected }: { kind: 'llm' | 'asr'; modelAccount: string; onModelSelected: () => void }) {
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
      await setCredential(modelAccount, model);
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
          <button onClick={loadModels} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.fetchModels')}</button>
          {models.length > 0 && (
            <SelectLite
              value={selectedModel}
              onChange={applyModel}
              disabled={status === 'loading'}
              options={models.map(model => ({ value: model, label: model }))}
              placeholder={t('settings.providers.selectModel')}
              ariaLabel={t('settings.providers.selectModel')}
              style={{ ...inputStyle, flex: mobile ? '1 1 100%' : '1 1 180px', maxWidth: mobile ? '100%' : 220 }}
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
  if (message === 'providerResponseTooLarge') return t('settings.providers.responseTooLarge');
  if (message === 'asrInvalidJson') return t('settings.providers.asrInvalidJson');
  if (message === 'asrMissingTextField') return t('settings.providers.asrMissingTextField');
  if (message === 'providerNetworkError') return t('common.networkError');
  if (message === 'providerReadResponseFailed' || message === 'providerClientInitFailed') return t('common.operationFailed');
  if (message === 'providerRequestTimeout') return t('settings.providers.requestTimeout');
  if (message.includes('API Key')) return t('settings.providers.apiKeyMissing');
  if (message.includes('Endpoint')) return t('settings.providers.endpointMissing');
  if (message.includes('timeout') || message.includes('超时')) return t('settings.providers.requestTimeout');
  return t('common.operationFailed');
}

type CredentialFieldStatus = 'idle' | 'saving' | 'saved' | 'readError' | 'saveError' | 'copied' | 'copyError';

interface CredentialFieldProps {
  label: string;
  account: string;
  placeholder?: string;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
  trailing?: ReactNode;
}

function CredentialField({ label, account, placeholder, mono, mask, defaultValue, trailing }: CredentialFieldProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<CredentialFieldStatus>('idle');
  const debounceRef = useRef<number | null>(null);
  const statusRef = useRef<number | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setDirty(false);
    setStatus('idle');
    setValue('');
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    readCredential(account)
      .then(v => {
        if (cancelled) return;
        setValue(v ?? '');
        setLoaded(true);
      })
      .catch(error => {
        if (cancelled) return;
        console.error('[settings] failed to read credential', account, error);
        setLoaded(true);
        setStatus('readError');
      });
    return () => {
      cancelled = true;
    };
  }, [account]);

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
      await setCredential(account, v);
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
          <input
            type={inputType}
            value={value}
            placeholder={loaded ? placeholder : t('common.loading')}
            onChange={handleChange}
            onBlur={onBlur}
            disabled={disabled}
            style={{ ...inputStyle, flex: mobile ? '1 1 180px' : 1, minWidth: 0, maxWidth: '100%', fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
          />
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
