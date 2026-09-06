// 服务 → AI 提供商：LLM 润色模型 + ASR 语音转写两张卡片。
// 自 Settings.tsx 整体迁出，逻辑零改动；i18n key 全部保持 `settings.providers.*`。

import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { detectOS } from '../../components/WindowChrome';
import {
  listProviderModels,
  listProviderDescriptors,
  readCredential,
  recordChannelTest,
  setActiveOmniProvider,
  setCredential,
  validateProviderCredentials,
  type ProviderDescriptor,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useLayoutStack, useConservativeLayout } from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite, type SelectOption } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import {
  SettingRow,
  SectionTitle,
  Toggle,
  inputStyle,
  segmentedTrackStyle,
} from './shared';
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
  'local-qwen3-mlx',
  'local-qwen3-c',
  'local-whisper',
  'foundry-local-whisper',
  'sherpa-onnx-local',
]);
function isLocalAsrPreset(id: string): boolean {
  return LOCAL_ASR_PRESET_IDS.has(id);
}

function LlmThinkingToggle({ enabled, onToggle }: { enabled: boolean; onToggle: (next: boolean) => void }) {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
  return (
    <div
      title={t('settings.providers.thinkingModeHint')}
      style={{
        display: 'flex',
        alignItems: 'center',
        flex: layoutStack ? '1 1 100%' : undefined,
        flexWrap: layoutStack ? 'wrap' : 'nowrap',
        gap: 6,
        paddingLeft: 2,
        whiteSpace: layoutStack ? 'normal' : 'nowrap',
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

// React 只保留本地化标签。endpoint、model、auth 与能力必须来自 Core
// ProviderDescriptor，避免每个平台各维护一份会漂移的业务真相。
export const LLM_LABELS = [
  ['ark', 'ark'], ['deepseek', 'deepseek'], ['siliconflow', 'siliconflow'],
  ['atlascloud', 'atlascloud'], ['openai', 'openai'], ['gemini', 'gemini'],
  ['codex_oauth', 'codexOAuth'], ['mimo', 'mimo'], ['cometapi', 'cometapi'],
  ['openrouterFree', 'openrouterFree'], ['alibabaCoding', 'alibabaCoding'],
  ['codingPlanX', 'codingPlanX'], ['minimax', 'minimax'], ['stepfun', 'stepfun'],
  ['custom', 'custom'],
].map(([id, nameKey]) => ({ id, nameKey })) as readonly { id: string; nameKey: string }[];

// 多模态（Omni）模型预设（issue #902）：一个模型同时接收「提示词 + 音频」一步输出
// 最终文本。凭据走独立 `omni.*` 命名空间，与上方 LLM/ASR 两套配置完全隔离。
// - openai       : OpenAI 官方（gpt-4o-audio-preview 等，input_audio part）
// - gemini       : Gemini 原生 generateContent（inlineData audio/wav）
// - dashscope-omni: 阿里云百炼 OpenAI 兼容通道（qwen3-omni-flash 等）
// - custom       : 任意 OpenAI 兼容多模态网关
const ASR_DEFAULT_RESOURCE_ID = 'volc.seedasr.sauc.duration';

/** 模型预设下拉里的「自定义模型…」哨兵值：选中即切回输入框手输。 */
const CUSTOM_MODEL_OPTION_VALUE = '__custom_model__';

/**
 * 一张渠道卡片的凭据字段区（编辑弹窗的主体）。
 *
 * 渠道化之前这里是「下拉选厂商 + 一组字段」；现在厂商由卡片自身的 providerType
 * 决定，字段一律按 `channelId` 作用域读写（后端 read_credential/set_credential 的
 * `provider` 参数收的就是渠道 id），因此同一家厂商的多张卡片互不干扰。
 */
export function ChannelCredentialFields({
  kind,
  providerType,
  channelId,
  descriptor,
  onTested,
  onUserMutation,
}: {
  kind: 'llm' | 'asr';
  providerType: string;
  channelId: string;
  descriptor?: Partial<Pick<ProviderDescriptor, 'authRequirement' | 'defaultEndpoint' | 'defaultModel' | 'staticModels'>>;
  /** 测试连通出结果后通知外层刷新卡片上的延迟/标红。 */
  onTested?: () => void;
  /** 新建草稿发生用户交互时同步通知外层，避免关闭流程误删。 */
  onUserMutation?: () => void;
}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
  const [llmModelRevision, setLlmModelRevision] = useState(0);
  const [asrModelRevision, setAsrModelRevision] = useState(0);
  const unifiedBailian = providerType === 'bailian';
  const [bailianModel, setBailianModel] = useState('');
  const [volcengineAuthMode, setVolcengineAuthMode] = useState<'app_id_token' | 'api_key'>('app_id_token');

  useEffect(() => {
    if (providerType === 'volcengine') {
      readCredential('volcengine.auth_mode', channelId)
        .then(v => {
          if (v === 'api_key') setVolcengineAuthMode('api_key');
          else setVolcengineAuthMode('app_id_token');
        })
        .catch(() => setVolcengineAuthMode('app_id_token'));
    }
  }, [providerType, channelId]);

  useEffect(() => {
    if (!unifiedBailian) setBailianModel('');
  }, [unifiedBailian]);

  const onLlmThinkingToggle = (enabled: boolean) => {
    if (!prefs) return;
    void updatePrefs(current => ({ ...current, llmThinkingEnabled: enabled })).catch(error => {
      console.error('[settings] failed to update LLM thinking mode', error);
      emitSaved('failed', t('common.operationFailed'));
    });
  };

  // Provider policy 必须 fail-closed：Core descriptor 尚未返回或加载失败时，
  // 不短暂渲染一套猜测的凭据字段，避免用户把秘密写进错误槽位。
  if (!descriptor) {
    return <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>;
  }

  if (kind === 'llm') {
    const defaultEndpoint = descriptor?.defaultEndpoint;
    const defaultModel = descriptor?.defaultModel;
    const codexOAuthSelected = descriptor?.authRequirement === 'o_auth';
    return (
      <>
        {codexOAuthSelected ? (
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, margin: '2px 0 10px' }}>
            {t('settings.providers.codexOAuthNotice')}
          </div>
        ) : (
          <>
            <CredentialField key={`${channelId}:api_key`} label={t('settings.providers.apiKeyLabel')}
              account="ark.api_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
            <CredentialField key={`${channelId}:endpoint`} label={t('settings.providers.baseUrlLabel')}
              account="ark.endpoint" provider={channelId}
              placeholder={defaultEndpoint || 'https://your-endpoint/v1'}
              defaultValue={defaultEndpoint || undefined} onUserMutation={onUserMutation} />
            {providerType === 'custom' && (
              <>
                <CredentialField
                  key={`${channelId}:temperature`}
                  label={t('settings.providers.temperatureLabel')}
                  account="ark.temperature"
                  placeholder={t('settings.providers.temperaturePlaceholder')}
                  mono
                  onUserMutation={onUserMutation}
                />
                <CredentialField
                  key={`${channelId}:extra_headers`}
                  label={t('settings.providers.extraHeadersLabel')}
                  account="ark.extra_headers"
                  placeholder={t('settings.providers.extraHeadersPlaceholder')}
                  mono
                  mask
                  onUserMutation={onUserMutation}
                />
              </>
            )}
          </>
        )}
        <CredentialField key={`${channelId}:model:${llmModelRevision}`} label={t('settings.providers.modelLabel')}
          account="ark.model_id" provider={channelId}
          placeholder={defaultModel || 'model-name'} mono
          defaultValue={defaultModel || undefined}
          onUserMutation={onUserMutation}
          trailing={(
            <LlmThinkingToggle
              enabled={prefs?.llmThinkingEnabled ?? false}
              onToggle={onLlmThinkingToggle}
            />
          )}
        />
        <ProviderTools kind="llm" modelAccount="ark.model_id" provider={channelId}
          onModelSelected={() => setLlmModelRevision(v => v + 1)} onTested={onTested}
          onUserMutation={onUserMutation} />
      </>
    );
  }

  const defaultEndpoint = descriptor?.defaultEndpoint;
  const defaultModel = descriptor?.defaultModel;

  if (descriptor?.authRequirement === 'volcengine') {
    return (
      <>
        <SettingRow label={t('settings.providers.volcengineAuthModeLabel')}>
          <SelectLite
            value={volcengineAuthMode}
            onChange={async (v) => {
              onUserMutation?.();
              const mode = v as 'app_id_token' | 'api_key';
              const prev = volcengineAuthMode;
              setVolcengineAuthMode(mode);
              try {
                await setCredential('volcengine.auth_mode', mode, channelId);
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
            style={{ ...inputStyle, width: '100%', maxWidth: layoutStack ? '100%' : 260 }}
          />
        </SettingRow>
        {/* 两种模式使用各自独立的凭据槽位：旧版 Access Token（volcengine.access_key）
            与方舟 API Key（volcengine.api_key）互不预填，切换模式不会残留混淆。 */}
        {volcengineAuthMode === 'app_id_token' ? (
          <>
            <CredentialField key={`${channelId}:app_key`} label={t('settings.providers.volcengineAppKeyLabel')}
              account="volcengine.app_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
            <CredentialField key={`${channelId}:access_key`} label={t('settings.providers.volcengineAccessKeyLabel')}
              account="volcengine.access_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
          </>
        ) : (
          <CredentialField key={`${channelId}:api_key`} label={t('settings.providers.volcengineApiKeyLabel')}
            account="volcengine.api_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
        )}
        <CredentialField
          key={`${channelId}:resource_id`}
          label={t('settings.providers.volcengineResourceIdLabel')}
          account="volcengine.resource_id"
          provider={channelId}
          mono
          onUserMutation={onUserMutation}
          placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
        <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {volcengineAuthMode === 'api_key'
            ? t('settings.providers.volcengineApiKeyNote')
            : t('settings.providers.volcengineMappingNote')}
        </div>
        <ProviderTools kind="asr" modelAccount="asr.model" provider={channelId}
          showFetchModels={false} onModelSelected={() => setAsrModelRevision(v => v + 1)} onTested={onTested}
          onUserMutation={onUserMutation} />
      </>
    );
  }

  if (descriptor?.authRequirement === 'xfyun') {
    return (
      <>
        <CredentialField key={`${channelId}:app_id`} label={t('settings.providers.xfyunAppIdLabel')}
          account="xfyun.app_id" provider={channelId} mono onUserMutation={onUserMutation} />
        <CredentialField key={`${channelId}:api_key`} label={t('settings.providers.xfyunApiKeyLabel')}
          account="xfyun.api_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
        <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.xfyunNote')}
        </div>
        <ProviderTools kind="asr" modelAccount="asr.model" provider={channelId}
          showFetchModels={false} onModelSelected={() => setAsrModelRevision(v => v + 1)} onTested={onTested}
          onUserMutation={onUserMutation} />
      </>
    );
  }

  // 本地引擎（qwen3 / sherpa / foundry / Apple 语音）没有 key 与地址；模型的下载与
  // 切换仍由「高级 → 本地模型」里的 <LocalAsr embedded /> 负责，这里只说明一句。
  if (descriptor?.authRequirement === 'none') {
    return (
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
        {t('settings.providers.localEngineNoCredentials')}
      </div>
    );
  }

  return (
    <>
      <CredentialField key={`${channelId}:api_key`} label={t('settings.providers.apiKeyLabel')}
        account="asr.api_key" provider={channelId} mono mask onUserMutation={onUserMutation} />
      {/* 统一百炼保留 endpoint 供用户选择区域或工作空间域名；后端按模型转换协议与路径。 */}
      <CredentialField key={`${channelId}:endpoint`} label={t('settings.providers.baseUrlLabel')}
        account="asr.endpoint" provider={channelId}
        placeholder={defaultEndpoint || 'https://your-endpoint/v1'}
        defaultValue={defaultEndpoint || undefined} onUserMutation={onUserMutation} />
      <CredentialField key={`${channelId}:model:${asrModelRevision}`} label={t('settings.providers.modelLabel')}
        account="asr.model" provider={channelId}
        placeholder={defaultModel || 'model-name'}
        defaultValue={defaultModel || undefined}
        onUserMutation={onUserMutation}
        onValueChange={unifiedBailian ? setBailianModel : undefined}
        options={descriptor?.staticModels?.length
          ? descriptor.staticModels.map(model => ({ value: model, label: model }))
          : undefined} />
      {unifiedBailian && (
        <BailianProtocolHint key={`${channelId}:proto:${asrModelRevision}`} currentModel={bailianModel} />
      )}
      {unifiedBailian && bailianModelSupportsVocabulary(bailianModel) && (
        <>
          <CredentialField
            key={`${channelId}:vocabulary_id`}
            label={t('settings.providers.bailianVocabularyIdLabel')}
            account="asr.vocabulary_id"
            provider={channelId}
            mono
            onUserMutation={onUserMutation}
            placeholder="vocab-..."
          />
          <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
            {t('settings.providers.bailianVocabularyIdNote')}
          </div>
        </>
      )}
      {providerType === 'elevenlabs' && (
        <div role="note" style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.elevenLabsUploadNotice')}
        </div>
      )}
      {providerType === 'zenmux' && (
        <div role="note" style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.zenmuxVocabularyNote')}
        </div>
      )}
      {/* 统一百炼「拉取模型」只写 model，不覆盖用户选择的区域或工作空间 endpoint。 */}
      <ProviderTools kind="asr" modelAccount="asr.model" provider={channelId}
        onModelSelected={() => setAsrModelRevision(v => v + 1)} onTested={onTested}
        onUserMutation={onUserMutation} />
      {(providerType === 'openai-compatible' || providerType === 'zenmux') && (
        <AsrAdvancedOptions provider={channelId} onUserMutation={onUserMutation} />
      )}
    </>
  );
}

// ASR 高级选项：openai-compatible 与 zenmux 两个预设显示。
// openai-compatible 暴露 verbose_json / 分片时长（其余命名厂商保持硬编码行为）；
// zenmux 暴露 enable_itn（数字归一化）开关，verbose_json / 分片对其无意义。
function AsrAdvancedOptions({
  provider,
  onUserMutation,
}: {
  provider: string;
  onUserMutation?: () => void;
}) {
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
    onUserMutation?.();
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

function ProviderTools({ kind, modelAccount, provider, onModelSelected, onTested, onUserMutation, showFetchModels = true }: { kind: 'llm' | 'asr' | 'omni'; modelAccount: string; provider?: string; onModelSelected: () => void; onTested?: () => void; onUserMutation?: () => void; showFetchModels?: boolean }) {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [status, setStatus] = useState<ProviderToolStatus>('idle');
  const [message, setMessage] = useState('');

  const setResult = (next: ProviderToolStatus, nextMessage: string) => {
    setStatus(next);
    setMessage(nextMessage);
  };

  // 把测试结果落到渠道上（卡片据此显示延迟或标红）。失败不打断主流程：
  // 测试本身已经在按钮旁给出结论，记录不上只是卡片少一行历史。
  const persistTest = async (ok: boolean, latencyMs: number | null, message: string | null) => {
    // Omni 不走渠道化（独立命名空间），没有可落测试结果的渠道卡片。
    if (!provider || kind === 'omni') return;
    try {
      await recordChannelTest(kind, provider, ok, latencyMs, message);
      onTested?.();
    } catch (error) {
      console.error('[settings] failed to record channel test', error);
    }
  };

  const validate = async () => {
    onUserMutation?.();
    setModels([]);
    setSelectedModel('');
    setResult('loading', t('settings.providers.validating'));
    const started = performance.now();
    try {
      const result = await validateProviderCredentials(kind, provider);
      const latency = Math.round(performance.now() - started);
      setResult(
        result.ok ? 'success' : 'error',
        t(result.ok ? 'settings.providers.validateSuccess' : 'settings.providers.validateFailed'),
      );
      await persistTest(result.ok, result.ok ? latency : null, result.ok ? null : 'validateFailed');
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if ((kind === 'llm' && message === 'llmModelMissing') || (kind === 'asr' && message === 'asrModelMissing')) {
        setResult('empty', t('settings.providers.modelMissing'));
        await persistTest(false, null, message);
        return;
      }
      if (message === 'modelsEmpty') {
        setResult('empty', t('settings.providers.modelsEmpty'));
        await persistTest(false, null, message);
        return;
      }
      setResult('error', providerErrorMessage(error, t));
      await persistTest(false, null, message);
    }
  };

  const loadModels = async () => {
    onUserMutation?.();
    setResult('loading', t('settings.providers.loadingModels'));
    try {
      const result = await listProviderModels(kind, provider);
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
    onUserMutation?.();
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
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%', maxWidth: layoutStack ? '100%' : 420 }}>
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
              style={{ flex: layoutStack ? '1 1 100%' : '1 1 180px', maxWidth: layoutStack ? '100%' : 220, minWidth: 0 }}
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
  if (message === 'volcengineAppIdMissing') return t('settings.providers.volcengineAppIdMissing');
  if (message === 'volcengineAccessTokenMissing') return t('settings.providers.volcengineAccessTokenMissing');
  if (message === 'volcengineApiKeyMissing') return t('settings.providers.apiKeyMissing');
  // 火山握手被拒/被限流的报错自带状态码与场景说明，原样透传比笼统的「操作失败」有用。
  if (message.includes('凭据被拒') || message.includes('被限流')) return message;
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
  /** 只在用户直接改变该字段时触发；初始化读取、复制和显隐不触发。 */
  onUserMutation?: () => void;
  /** 提供则渲染为下拉（预设选择）代替输入框；当前值不在预设里时附加为自定义项。 */
  options?: SelectOption[];
}

function CredentialField({ label, account, provider, placeholder, mono, mask, defaultValue, trailing, onValueChange, onUserMutation, options }: CredentialFieldProps) {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
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
    onUserMutation?.();
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
    onUserMutation?.();
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
  const showInsecureEndpointWarning = (account === 'ark.endpoint' || account === 'asr.endpoint' || account === 'omni.endpoint')
    && value.trim().toLowerCase().startsWith('http://');

  return (
    <SettingRow label={label}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 5, width: '100%', maxWidth: layoutStack ? '100%' : 420 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', flexWrap: layoutStack ? 'wrap' : 'nowrap' }}>
          {options && !customModelMode ? (
            <SelectLite
              value={value}
              onChange={(v) => {
                // 「自定义模型…」逃生口：切回输入框手输任意模型名。
                if (v === CUSTOM_MODEL_OPTION_VALUE) {
                  setCustomModelMode(true);
                  return;
                }
                onUserMutation?.();
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
              style={{ flex: layoutStack ? '1 1 180px' : 1, minWidth: 0, maxWidth: '100%', fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
            />
          ) : (
            <input
              type={inputType}
              value={value}
              placeholder={loaded ? placeholder : t('common.loading')}
              onChange={handleChange}
              onBlur={onBlur}
              disabled={disabled}
              style={{ ...inputStyle, flex: layoutStack ? '1 1 180px' : 1, minWidth: 0, maxWidth: '100%', fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
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
        {showInsecureEndpointWarning && (
          <span style={{ fontSize: 11, color: 'var(--ol-warn)', lineHeight: 1.45 }}>
            {t('settings.providers.endpointHttpWarning')}
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

/**
 * 多模态（Omni）配置卡片：仅在「高级 → 实验性」里打开多模态管线后出现。
 *
 * 它不参与渠道排序——Omni 是独立命名空间，渠道化范围只覆盖 ASR/LLM
 * （见 docs/provider-channels-plan.md 的分期）；管道模式切换沿用既有语义：
 * 多模态模式下隐藏传统 llm/asr 渠道列表，凭据两套并存但停用，切回即恢复。
 */
export function OmniChannelSection() {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
  const { prefs, updatePrefs } = useHotkeySettings();
  const [descriptors, setDescriptors] = useState<ProviderDescriptor[]>([]);
  const [omniProvider, setOmniProvider] = useState('custom');
  const [committedOmniProvider, setCommittedOmniProvider] = useState('custom');
  const omniSwitchSeqRef = useRef(0);
  const [omniModelRevision, setOmniModelRevision] = useState(0);

  useEffect(() => {
    void listProviderDescriptors('omni')
      .then(setDescriptors)
      .catch(error => console.error('[settings] failed to load omni provider descriptors', error));
  }, []);

  const omniPresets = useMemo(() => descriptors.map(descriptor => ({
      id: descriptor.providerType,
      nameKey: descriptor.labelKey,
      baseUrl: descriptor.defaultEndpoint ?? '',
      modelPlaceholder: descriptor.defaultModel ?? '',
    })), [descriptors]);

  useEffect(() => {
    if (!prefs) return;
    const knownOmni = omniPresets.find(x => x.id === prefs.activeOmniProvider);
    const omniId = knownOmni ? knownOmni.id : 'custom';
    setOmniProvider(omniId);
    setCommittedOmniProvider(omniId);
  }, [prefs, omniPresets]);

  // 与 LLM 卡同语义：受控下拉立即反馈 + committed 控制 CredentialField remount
  // + seq 守卫防 stale 覆盖，只是凭据落到 omni.* 槽。
  const onOmniProviderChange = async (id: string) => {
    setOmniProvider(id);
    const seq = ++omniSwitchSeqRef.current;
    emitSaved('saving', t('common.saving'));
    let backendSwitched = false;
    try {
      await setActiveOmniProvider(id);
      backendSwitched = true;
      if (seq !== omniSwitchSeqRef.current) return;
      if (prefs) {
        const next = { ...prefs, activeOmniProvider: id };
        await updatePrefs(next);
        if (seq !== omniSwitchSeqRef.current) return;
      }
      const preset = omniPresets.find(p => p.id === id);
      // 切到非 custom 预设强制覆盖 endpoint/model 默认值（与 LLM 卡同语义），
      // 保证「切换」真切到位，不残留旧厂商的槽值。
      if (preset && preset.id !== 'custom') {
        if (preset.baseUrl) {
          await setCredential('omni.endpoint', preset.baseUrl);
          if (seq !== omniSwitchSeqRef.current) return;
        }
        if (preset.modelPlaceholder) {
          await setCredential('omni.model', preset.modelPlaceholder);
          if (seq !== omniSwitchSeqRef.current) return;
        }
      }
      setCommittedOmniProvider(id);
      emitSaved('saved', t('common.saved'));
    } catch (err) {
      if (seq === omniSwitchSeqRef.current) {
        emitSaved('failed', t('common.operationFailed'));
        if (!backendSwitched) {
          setOmniProvider(committedOmniProvider);
        }
      }
      console.error('[settings] switch omni provider failed', err);
    }
  };

  // 识别管线模式：切换只改偏好，不删除另一套凭据，切回即恢复；运行时只读当前模式。
  const onPipelineModeChange = (mode: 'traditional' | 'multimodal') => {
    if (!prefs) return;
    void updatePrefs(current => ({ ...current, pipelineMode: mode })).catch(error => {
      console.error('[settings] failed to update pipeline mode', error);
      emitSaved('failed', t('common.operationFailed'));
    });
  };

  if (prefs?.multimodalPipelineEnabled !== true) return null;
  const multimodalMode = prefs?.pipelineMode === 'multimodal';
  const omniPreset = omniPresets.find(p => p.id === committedOmniProvider);

  return (
    <>
      <div style={{ marginBottom: 12 }}>
        <SettingRow
          label={t('settings.providers.pipelineModeLabel')}
          desc={t('settings.providers.pipelineModeHint')}
        >
          <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: layoutStack ? 'wrap' : 'nowrap' }}>
            <div style={segmentedTrackStyle}>
              {(['traditional', 'multimodal'] as const).map(mode => (
                <button
                  key={mode}
                  onClick={() => onPipelineModeChange(mode)}
                  style={{
                    padding: '5px 12px', fontSize: 12, fontWeight: 500, border: 0, borderRadius: 6,
                    fontFamily: 'inherit',
                    background: prefs?.pipelineMode === mode ? 'var(--ol-segmented-active-bg)' : 'transparent',
                    color: prefs?.pipelineMode === mode ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                    boxShadow: prefs?.pipelineMode === mode ? 'var(--ol-segmented-active-shadow)' : 'none',
                    cursor: 'default',
                  }}
                >
                  {mode === 'traditional'
                    ? t('settings.providers.pipelineModeTraditional')
                    : t('settings.providers.pipelineModeMultimodal')}
                </button>
              ))}
            </div>
          </div>
        </SettingRow>
        <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', lineHeight: 1.5, paddingLeft: 2 }}>
          {t('settings.providers.pipelineIsolationNotice')}
        </div>
      </div>
      {multimodalMode && (
        <Card>
          <div style={{ marginBottom: 10 }}>
            <SectionTitle>{t('settings.providers.omniTitle')}</SectionTitle>
          </div>
          <SettingRow label={t('settings.providers.providerLabel')}>
            <SelectLite
              value={omniProvider}
              onChange={next => onOmniProviderChange(next)}
              options={omniPresets.map(p => ({
                value: p.id,
                label: t(`settings.providers.presets.${p.nameKey}`),
              }))}
              ariaLabel={t('settings.providers.providerLabel')}
              style={{ ...inputStyle, width: '100%', maxWidth: layoutStack ? '100%' : 200 }}
            />
          </SettingRow>
          <CredentialField
            key={`${committedOmniProvider}:api_key`}
            label={t('settings.providers.apiKeyLabel')}
            account="omni.api_key"
            mono
            mask
          />
          <CredentialField
            key={`${committedOmniProvider}:endpoint`}
            label={t('settings.providers.baseUrlLabel')}
            account="omni.endpoint"
            placeholder={omniPreset?.baseUrl || 'https://your-endpoint/v1'}
          />
          {committedOmniProvider === 'custom' && (
            <>
              <CredentialField
                key="omni:temperature"
                label={t('settings.providers.temperatureLabel')}
                account="omni.temperature"
                placeholder={t('settings.providers.temperaturePlaceholder')}
                mono
              />
              <CredentialField
                key="omni:extra_headers"
                label={t('settings.providers.extraHeadersLabel')}
                account="omni.extra_headers"
                placeholder={t('settings.providers.extraHeadersPlaceholder')}
                mono
                mask
              />
            </>
          )}
          <CredentialField
            key={`${committedOmniProvider}:model:${omniModelRevision}`}
            label={t('settings.providers.modelLabel')}
            account="omni.model"
            placeholder={omniPreset?.modelPlaceholder || 'model-name'}
            mono
          />
          <ProviderTools
            key={`omni:${committedOmniProvider}`}
            kind="omni"
            modelAccount="omni.model"
            onModelSelected={() => setOmniModelRevision(v => v + 1)}
          />
        </Card>
      )}
    </>
  );
}
