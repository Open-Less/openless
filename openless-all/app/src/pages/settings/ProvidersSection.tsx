// Channel credentials, model lists, and connection validation forms. Provider capabilities and
// defaults come from Core; the UI handles localization, field-save feedback, and user-triggered
// model fetching and validation.

import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import {
  listProviderModels,
  openExternal,
  listProviderDescriptors,
  readCredential,
  recordChannelTest,
  setActiveOmniProvider,
  setCredential,
  validateProviderCredentials,
  type ProviderDescriptor,
} from '../../lib/ipc';
import { BailianProtocolField } from './BailianProtocolField';
import type { BailianProtocol } from '../../lib/bailianProtocol';
import { LlmProtocolFields } from './LlmProtocolFields';
import { ProviderFormContext } from './ProviderForm';
import { LocalModelPicker } from './models/LocalModelPicker';
import { emitSaved } from '../../lib/savedEvent';
import { useContentMotion } from '../../lib/motion';
import { useLayoutStack, useConservativeLayout } from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { SelectLite, type SelectOption } from '../../components/ui/SelectLite';
import { Card } from '../_atoms';
import { SettingRow, SectionTitle, Toggle, inputStyle } from './shared';
import {
  parseAdvancedAsrConfig,
  serializeAdvancedAsrConfig,
  type AdvancedAsrConfig,
} from '../../lib/advancedAsrConfig';
/** Channel-local form layout; do not change the shared settings row contract. */
export function ChannelFormRow({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string;
  children: ReactNode;
}) {
  const stack = useLayoutStack();
  const conservative = useConservativeLayout();
  return (
    <div className="ol-channel-form-row" data-stacked={stack || conservative ? 'true' : undefined}>
      <label htmlFor={htmlFor}>{label}</label>
      <div style={{ minWidth: 0, width: '100%' }}>{children}</div>
    </div>
  );
}

export function ChannelSectionHeading({
  title,
  description,
  icon,
}: {
  title: string;
  description?: string;
  icon?: string;
}) {
  return (
    <div className="ol-channel-section-heading" style={{ marginBottom: 6 }}>
      <h3 style={{ margin: 0, fontSize: 14, color: 'var(--ol-ink)', fontWeight: 600 }}>
        {icon && <Icon name={icon} size={15} />}
        {title}
      </h3>
      {description && (
        <p style={{ margin: '6px 0 0', color: 'var(--ol-ink-3)', fontSize: 12, lineHeight: 1.65 }}>
          {description}
        </p>
      )}
    </div>
  );
}

const channelSectionStyle: CSSProperties = {
  borderTop: '1px solid var(--ol-line)',
  marginTop: 24,
  paddingTop: 20,
};

function LlmThinkingToggle({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: (next: boolean) => void;
}) {
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

// React holds only localization labels. endpoint, model, auth, and capabilities must come from
// the Core ProviderDescriptor, so each platform does not maintain its own drifting copy of
// business truth.
export const LLM_LABELS = [
  ['ark', 'ark'],
  ['deepseek', 'deepseek'],
  ['siliconflow', 'siliconflow'],
  ['atlascloud', 'atlascloud'],
  ['openai', 'openai'],
  ['gemini', 'gemini'],
  ['codex_oauth', 'codexOAuth'],
  ['mimo', 'mimo'],
  ['cometapi', 'cometapi'],
  ['openrouterFree', 'openrouterFree'],
  ['requesty', 'requesty'],
  ['api-route', 'apiRoute'],
  ['orcarouter', 'orcarouter'],
  ['alibabaCoding', 'alibabaCoding'],
  ['codingPlanX', 'codingPlanX'],
  ['minimax', 'minimax'],
  ['stepfun', 'stepfun'],
  ['opencode', 'opencode'],
  ['tencentTokenHub', 'tencentTokenHub'],
  ['lmstudio', 'lmstudio'],
  ['custom', 'customChatCompletions'],
  ['custom_responses', 'customResponses'],
  ['custom_messages', 'customMessages'],
].map(([id, nameKey]) => ({ id, nameKey })) as readonly { id: string; nameKey: string }[];

// Default shown for Volcano speech transcription when the resource ID is empty.
const ASR_DEFAULT_RESOURCE_ID = 'volc.seedasr.sauc.duration';

/** Sentinel value for the "Custom model…" entry in the model preset dropdown; selecting it switches back to a manual input. */
const CUSTOM_MODEL_OPTION_VALUE = '__custom_model__';

function matchesEndpointPreset(value: string, endpoint: string): boolean {
  try {
    const current = new URL(value.trim());
    const preset = new URL(endpoint);
    const basePath = (path: string) =>
      path
        .replace(/\/$/, '')
        .replace(/\/(chat\/completions|responses|messages|models)$/, '')
        .replace(/\/v3$/, '');
    return (
      current.origin === preset.origin &&
      !current.search &&
      !current.hash &&
      !current.username &&
      !current.password &&
      basePath(current.pathname) === basePath(preset.pathname)
    );
  } catch {
    return false;
  }
}

/**
 * Credential field area of one channel card (the body of the edit dialog).
 *
 * Before channelization this was "dropdown of vendors + a group of fields"; now the vendor is
 * determined by the card's own providerType and fields are always read/written scoped by
 * `channelId` (the `provider` parameter of the backend read_credential/set_credential takes the
 * channel id), so multiple cards for the same vendor do not interfere.
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
  descriptor?: Partial<
    Pick<
      ProviderDescriptor,
      | 'authRequirement'
      | 'defaultEndpoint'
      | 'endpointPresets'
      | 'defaultModel'
      | 'staticModels'
      | 'defaultRequestFormat'
      | 'supportedRequestFormats'
    >
  >;
  /** Notify the parent to refresh the card's latency / error highlight after a connectivity test resolves. */
  onTested?: () => void;
  /** Notify the parent whenever a new draft sees user interaction, so the close flow does not discard it. */
  onUserMutation?: () => void;
}) {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [llmEndpoint, setLlmEndpoint] = useState('');
  const [llmModelRevision, setLlmModelRevision] = useState(0);
  const [configRevision, setConfigRevision] = useState(0);
  const [orcarouterCatalogRevision, setOrcarouterCatalogRevision] = useState(0);
  const [blockedFields, setBlockedFields] = useState<Record<string, boolean>>({});
  const trackField = useCallback((account: string, blocked: boolean) => {
    setBlockedFields((previous) =>
      previous[account] === blocked ? previous : { ...previous, [account]: blocked },
    );
  }, []);
  const onLlmMutation = () => {
    onUserMutation?.();
    setConfigRevision((value) => value + 1);
  };

  const [asrModelRevision, setAsrModelRevision] = useState(0);
  const unifiedBailian = providerType === 'bailian';
  const [bailianModel, setBailianModel] = useState('');
  const [bailianProtocol, setBailianProtocol] = useState<BailianProtocol>('auto');
  const [volcengineAuthMode, setVolcengineAuthMode] = useState<'app_id_token' | 'api_key'>(
    'app_id_token',
  );

  const providerForm = useContext(ProviderFormContext);
  const formTrack = providerForm?.track;
  const trackVolcengineSetting = useCallback(
    (account: string, blocked: boolean) => {
      trackField(account, blocked);
      formTrack?.(account, blocked);
    },
    [trackField, formTrack],
  );
  const [volcengineService, setVolcengineService] = useState('standard');
  const onAsrMutation = () => {
    onUserMutation?.();
    setConfigRevision((value) => value + 1);
  };

  useEffect(() => {
    if (providerType !== 'volcengine') return;
    let cancelled = false;
    trackField('volcengine.config', true);
    Promise.all([
      readCredential('volcengine.service', channelId),
      readCredential('volcengine.auth_mode', channelId),
    ])
      .then(([service, mode]) => {
        if (cancelled) return;
        setVolcengineService(service || 'standard');
        setVolcengineAuthMode(mode === 'api_key' ? 'api_key' : 'app_id_token');
        trackField('volcengine.config', false);
      })
      .catch(() => {
        if (!cancelled) emitSaved('failed', t('common.operationFailed'));
      });
    return () => {
      cancelled = true;
    };
  }, [providerType, channelId, trackField, t]);

  useEffect(() => {
    if (!unifiedBailian) setBailianModel('');
  }, [unifiedBailian]);

  const onLlmThinkingToggle = (enabled: boolean) => {
    if (!prefs) return;
    onLlmMutation();
    trackField('thinking', true);
    void updatePrefs((current) => ({ ...current, llmThinkingEnabled: enabled }))
      .then(() => onTested?.())
      .catch((error) => {
        console.error('[settings] failed to update LLM thinking mode', error);
        emitSaved('failed', t('common.operationFailed'));
      })
      .finally(() => trackField('thinking', false));
  };

  // Provider policy must fail closed: while the Core descriptor has not loaded or failed to
  // load, do not briefly render a guessed set of credential fields — users could type secrets
  // into the wrong slot.
  if (!descriptor) {
    return <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>;
  }

  if (kind === 'llm') {
    const defaultEndpoint = descriptor?.defaultEndpoint;
    const defaultModel = descriptor?.defaultModel;
    const modelsUrl = descriptor.endpointPresets?.find((preset) =>
      matchesEndpointPreset(llmEndpoint || defaultEndpoint || '', preset.endpoint),
    )?.modelsUrl;
    const codexOAuthSelected = descriptor?.authRequirement === 'o_auth';
    return (
      <>
        {!!descriptor.supportedRequestFormats?.length && descriptor.defaultRequestFormat && (
          <LlmProtocolFields
            channelId={channelId}
            defaultFormat={descriptor.defaultRequestFormat}
            formats={descriptor.supportedRequestFormats}
            onUserMutation={onLlmMutation}
            onBlockedChange={trackField}
            onSaved={(changedAccounts) => {
              if (
                providerType === 'orcarouter' &&
                changedAccounts?.includes('ark.request_format')
              ) {
                setOrcarouterCatalogRevision((value) => value + 1);
              }
              onTested?.();
            }}
          />
        )}
        {codexOAuthSelected ? (
          <div
            style={{
              fontSize: 11.5,
              color: 'var(--ol-ink-4)',
              lineHeight: 1.6,
              margin: '2px 0 10px',
            }}
          >
            {t('settings.providers.codexOAuthNotice')}
          </div>
        ) : (
          <>
            <CredentialField
              key={`${channelId}:api_key`}
              label={t(
                descriptor.authRequirement === 'endpoint_model_optional_api_key'
                  ? 'settings.providers.apiKeyOptionalLabel'
                  : 'settings.providers.apiKeyLabel',
              )}
              account="ark.api_key"
              provider={channelId}
              mono
              mask
              onUserMutation={onLlmMutation}
              onBlockedChange={trackField}
            />
            <CredentialField
              key={`${channelId}:endpoint`}
              label={t('settings.providers.baseUrlLabel')}
              account="ark.endpoint"
              onValueChange={setLlmEndpoint}
              endpointPresets={
                providerType === 'ark' && defaultEndpoint && descriptor.endpointPresets?.length
                  ? {
                      label: t('settings.providers.volcengineServiceLabel'),
                      options: [
                        { value: defaultEndpoint, label: t('settings.providers.presets.ark') },
                        ...descriptor.endpointPresets.map(({ name, endpoint }) => ({
                          value: endpoint,
                          label: name,
                        })),
                      ],
                    }
                  : undefined
              }
              provider={channelId}
              placeholder={defaultEndpoint || 'https://your-endpoint/v1'}
              defaultValue={defaultEndpoint || undefined}
              onUserMutation={onLlmMutation}
              onBlockedChange={trackField}
            />
            {['custom', 'custom_responses', 'custom_messages'].includes(providerType) && (
              <>
                <CredentialField
                  key={`${channelId}:extra_headers`}
                  label={t('settings.providers.extraHeadersLabel')}
                  account="ark.extra_headers"
                  provider={channelId}
                  placeholder={t('settings.providers.extraHeadersPlaceholder')}
                  mono
                  mask
                  onUserMutation={onLlmMutation}
                  onBlockedChange={trackField}
                />
              </>
            )}
          </>
        )}
        <div style={channelSectionStyle}>
          <ChannelSectionHeading
            icon="settings"
            title={t('settings.channels.modelTitle')}
            description={t(
              modelsUrl ? 'settings.providers.planModelsHint' : 'settings.channels.modelHint',
            )}
          />
        </div>
        {providerType === 'orcarouter' ? (
          <CatalogModelField
            key={`${channelId}:catalog:${orcarouterCatalogRevision}`}
            kind="llm"
            provider={channelId}
            baseUrl={defaultEndpoint ?? ''}
            defaultModel={defaultModel ?? ''}
            onUserMutation={onLlmMutation}
            onBlockedChange={trackField}
            trailing={
              <LlmThinkingToggle
                enabled={prefs?.llmThinkingEnabled ?? false}
                onToggle={onLlmThinkingToggle}
              />
            }
          />
        ) : (
          <CredentialField
            key={`${channelId}:model:${llmModelRevision}`}
            label={t('settings.providers.modelLabel')}
            account="ark.model_id"
            provider={channelId}
            placeholder={defaultModel || 'model-name'}
            mono
            defaultValue={defaultModel || undefined}
            onUserMutation={onLlmMutation}
            onBlockedChange={trackField}
            trailing={
              <LlmThinkingToggle
                enabled={prefs?.llmThinkingEnabled ?? false}
                onToggle={onLlmThinkingToggle}
              />
            }
          />
        )}
        {['custom', 'custom_responses', 'custom_messages'].includes(providerType) && (
          <CredentialField
            key={`${channelId}:temperature`}
            label={t('settings.providers.temperatureLabel')}
            account="ark.temperature"
            provider={channelId}
            placeholder={t('settings.providers.temperaturePlaceholder')}
            mono
            onUserMutation={onLlmMutation}
            onBlockedChange={trackField}
          />
        )}
        <ProviderTools
          key={configRevision}
          disabled={
            Object.values(blockedFields).some(Boolean) ||
            (!!descriptor.supportedRequestFormats?.length && blockedFields.protocol === undefined)
          }
          kind="llm"
          modelAccount="ark.model_id"
          modelsUrl={modelsUrl}
          provider={channelId}
          onModelSelected={() => setLlmModelRevision((v) => v + 1)}
          onTested={onTested}
          onUserMutation={onUserMutation}
          showFetchModels={providerType !== 'orcarouter'}
        />
      </>
    );
  }

  const defaultEndpoint = descriptor?.defaultEndpoint;
  const defaultModel = descriptor?.defaultModel;

  if (descriptor?.authRequirement === 'volcengine') {
    const agentPlan = volcengineService === 'agent_plan';
    const configBlocked = blockedFields['volcengine.config'] !== false;
    const activeAccounts = [
      'volcengine.service',
      'volcengine.auth_mode',
      'volcengine.resource_id',
      ...(!agentPlan && volcengineAuthMode === 'app_id_token'
        ? ['volcengine.app_key', 'volcengine.access_key']
        : ['volcengine.api_key']),
    ];
    const blocked = configBlocked || activeAccounts.some((account) => blockedFields[account]);
    return (
      <>
        <ChannelFormRow label={t('settings.providers.volcengineServiceLabel')}>
          <SelectLite
            value={volcengineService}
            disabled={blocked}
            onChange={async (service) => {
              onAsrMutation();
              const previous = volcengineService;
              setVolcengineService(service);
              trackVolcengineSetting('volcengine.service', true);
              try {
                await setCredential('volcengine.service', service, channelId);
                onTested?.();
              } catch (error) {
                console.error('[settings] failed to save volcengine service', error);
                setVolcengineService(previous);
                emitSaved('failed', t('common.operationFailed'));
              } finally {
                trackVolcengineSetting('volcengine.service', false);
              }
            }}
            options={[
              { value: 'standard', label: t('settings.providers.volcengineServiceStandard') },
              { value: 'agent_plan', label: 'Agent Plan' },
            ]}
            ariaLabel={t('settings.providers.volcengineServiceLabel')}
            style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
          />
        </ChannelFormRow>
        {!agentPlan && (
          <ChannelFormRow label={t('settings.providers.volcengineAuthModeLabel')}>
            <SelectLite
              value={volcengineAuthMode}
              disabled={blocked}
              onChange={async (v) => {
                onAsrMutation();
                const mode = v as 'app_id_token' | 'api_key';
                const prev = volcengineAuthMode;
                setVolcengineAuthMode(mode);
                trackVolcengineSetting('volcengine.auth_mode', true);
                try {
                  await setCredential('volcengine.auth_mode', mode, channelId);
                  onTested?.();
                } catch (error) {
                  // A failed write must roll the UI back and show an error: otherwise the mode
                  // looks switched but silently reverts after restart, which combined with the
                  // separate API Key slot produces "key exists but mode is wrong" confusion.
                  console.error('[settings] failed to save volcengine auth mode', error);
                  setVolcengineAuthMode(prev);
                  emitSaved('failed', t('common.operationFailed'));
                } finally {
                  trackVolcengineSetting('volcengine.auth_mode', false);
                }
              }}
              options={[
                {
                  value: 'app_id_token',
                  label: t('settings.providers.volcengineAuthModeAppIdToken'),
                },
                { value: 'api_key', label: t('settings.providers.volcengineAuthModeApiKey') },
              ]}
              ariaLabel={t('settings.providers.volcengineAuthModeLabel')}
              style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
            />
          </ChannelFormRow>
        )}
        {/* The two modes use separate credential slots: the legacy Access Token
            (volcengine.access_key) and the ASR API Key (volcengine.api_key, used for regular
            service API key auth or Agent Plan) never prefill each other. */}
        {!agentPlan && volcengineAuthMode === 'app_id_token' ? (
          <>
            <CredentialField
              key={`${channelId}:app_key`}
              label={t('settings.providers.volcengineAppKeyLabel')}
              account="volcengine.app_key"
              provider={channelId}
              mono
              mask
              onUserMutation={onAsrMutation}
              onBlockedChange={trackField}
            />
            <CredentialField
              key={`${channelId}:access_key`}
              label={t('settings.providers.volcengineAccessKeyLabel')}
              account="volcengine.access_key"
              provider={channelId}
              mono
              mask
              onUserMutation={onAsrMutation}
              onBlockedChange={trackField}
            />
          </>
        ) : (
          <CredentialField
            key={`${channelId}:api_key`}
            label={t('settings.providers.volcengineApiKeyLabel')}
            account="volcengine.api_key"
            provider={channelId}
            mono
            mask
            onUserMutation={onAsrMutation}
            onBlockedChange={trackField}
          />
        )}
        <div style={channelSectionStyle}>
          <ChannelSectionHeading icon="settings" title={t('settings.channels.modelTitle')} />
        </div>
        <CredentialField
          key={`${channelId}:resource_id`}
          label={t('settings.providers.volcengineResourceIdLabel')}
          account="volcengine.resource_id"
          provider={channelId}
          mono
          onUserMutation={onAsrMutation}
          onBlockedChange={trackField}
          placeholder={ASR_DEFAULT_RESOURCE_ID}
          defaultValue={ASR_DEFAULT_RESOURCE_ID}
        />
        <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {agentPlan
            ? t('settings.providers.volcengineAgentPlanNote')
            : volcengineAuthMode === 'api_key'
              ? t('settings.providers.volcengineApiKeyNote')
              : t('settings.providers.volcengineMappingNote')}
        </div>
        <ProviderTools
          key={configRevision}
          disabled={blocked}
          kind="asr"
          modelAccount="asr.model"
          provider={channelId}
          showFetchModels={false}
          onModelSelected={() => setAsrModelRevision((v) => v + 1)}
          onTested={onTested}
          onUserMutation={onUserMutation}
        />
      </>
    );
  }

  if (descriptor?.authRequirement === 'xfyun') {
    return (
      <>
        <CredentialField
          key={`${channelId}:app_id`}
          label={t('settings.providers.xfyunAppIdLabel')}
          account="xfyun.app_id"
          provider={channelId}
          mono
          onUserMutation={onUserMutation}
        />
        <CredentialField
          key={`${channelId}:api_key`}
          label={t('settings.providers.xfyunApiKeyLabel')}
          account="xfyun.api_key"
          provider={channelId}
          mono
          mask
          onUserMutation={onUserMutation}
        />
        <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.xfyunNote')}
        </div>
        <ProviderTools
          kind="asr"
          modelAccount="asr.model"
          provider={channelId}
          showFetchModels={false}
          onModelSelected={() => setAsrModelRevision((v) => v + 1)}
          onTested={onTested}
          onUserMutation={onUserMutation}
        />
      </>
    );
  }

  // Tencent Cloud realtime speech: three-part keys + fixed model tier (Preview is 16kHz mono
  // only, under 60 seconds).
  if (descriptor?.authRequirement === 'tencent_cloud') {
    return (
      <>
        <CredentialField
          key={`${channelId}:app_id`}
          label={t('settings.providers.tencentCloudAppIdLabel')}
          account="tencent_cloud.app_id"
          provider={channelId}
          mono
          onUserMutation={onUserMutation}
        />
        <CredentialField
          key={`${channelId}:secret_id`}
          label={t('settings.providers.tencentCloudSecretIdLabel')}
          account="tencent_cloud.secret_id"
          provider={channelId}
          mono
          mask
          onUserMutation={onUserMutation}
        />
        <CredentialField
          key={`${channelId}:secret_key`}
          label={t('settings.providers.tencentCloudSecretKeyLabel')}
          account="tencent_cloud.secret_key"
          provider={channelId}
          mono
          mask
          onUserMutation={onUserMutation}
        />
        <div style={channelSectionStyle}>
          <ChannelSectionHeading icon="settings" title={t('settings.channels.modelTitle')} />
        </div>
        <CredentialField
          key={`${channelId}:model`}
          label={t('settings.providers.modelLabel')}
          account="asr.model"
          provider={channelId}
          mono
          placeholder={defaultModel || 'Hy-ASR-3.0-preview'}
          defaultValue={defaultModel || 'Hy-ASR-3.0-preview'}
          onUserMutation={onUserMutation}
        />
        <div style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.tencentCloudNote')}
        </div>
        <ProviderTools
          kind="asr"
          modelAccount="asr.model"
          provider={channelId}
          showFetchModels={false}
          onModelSelected={() => setAsrModelRevision((v) => v + 1)}
          onTested={onTested}
          onUserMutation={onUserMutation}
        />
      </>
    );
  }

  // Local engines (qwen3 / sherpa / foundry / Apple speech) have no key or address; model
  // download and switching stay with <LocalAsr embedded /> under "Advanced -> Local models",
  // so only a one-line note here.
  if (descriptor?.authRequirement === 'none') {
    // Local engine: the model is the credential. This switches the in-use model directly
    // (global effect); download / delete / mirror live in the "Services -> Local models" page.
    return (
      <>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}>
          {t('settings.providers.localEngineNoCredentials')}
        </div>
        <LocalModelPicker providerType={providerType} />
      </>
    );
  }

  if (providerType === 'orcarouter') {
    return (
      <>
        <CredentialField
          key={`${channelId}:api_key`}
          label={t('settings.providers.apiKeyLabel')}
          account="asr.api_key"
          provider={channelId}
          mono
          mask
          onUserMutation={onUserMutation}
        />
        <CredentialField
          key={`${channelId}:endpoint`}
          label={t('settings.providers.baseUrlLabel')}
          account="asr.endpoint"
          provider={channelId}
          placeholder={defaultEndpoint ?? undefined}
          defaultValue={defaultEndpoint ?? undefined}
          onUserMutation={onUserMutation}
        />
        <CatalogModelField
          kind="asr"
          provider={channelId}
          baseUrl={defaultEndpoint ?? ''}
          defaultModel={defaultModel ?? ''}
          onUserMutation={onUserMutation}
        />
        <ProviderTools
          kind="asr"
          modelAccount="asr.model"
          provider={channelId}
          onModelSelected={() => setAsrModelRevision((v) => v + 1)}
          onTested={onTested}
          onUserMutation={onUserMutation}
          showFetchModels={false}
        />
      </>
    );
  }

  return (
    <>
      <CredentialField
        key={`${channelId}:api_key`}
        label={t('settings.providers.apiKeyLabel')}
        account="asr.api_key"
        provider={channelId}
        mono
        mask
        onUserMutation={onUserMutation}
      />
      {/* Unified Bailian keeps the endpoint for users choosing a region or workspace domain; the backend converts protocol and path per model. */}
      <CredentialField
        key={`${channelId}:endpoint`}
        label={t('settings.providers.baseUrlLabel')}
        account="asr.endpoint"
        provider={channelId}
        placeholder={defaultEndpoint || 'https://your-endpoint/v1'}
        defaultValue={defaultEndpoint || undefined}
        onUserMutation={onUserMutation}
      />
      <div style={channelSectionStyle}>
        <ChannelSectionHeading
          icon="settings"
          title={t('settings.channels.modelTitle')}
          description={t('settings.channels.modelHint')}
        />
      </div>
      <CredentialField
        key={`${channelId}:model:${asrModelRevision}`}
        label={t('settings.providers.modelLabel')}
        account="asr.model"
        provider={channelId}
        placeholder={defaultModel || 'model-name'}
        defaultValue={defaultModel || undefined}
        onUserMutation={onUserMutation}
        onValueChange={unifiedBailian ? setBailianModel : undefined}
        options={
          descriptor?.staticModels?.length
            ? descriptor.staticModels.map((model) => ({ value: model, label: model }))
            : undefined
        }
      />
      {unifiedBailian && (
        <BailianProtocolField
          key={channelId}
          channelId={channelId}
          onChange={setBailianProtocol}
          onUserMutation={onAsrMutation}
          onBlockedChange={trackField}
        />
      )}
      {unifiedBailian && (
        <BailianProtocolHint
          key={`${channelId}:proto:${asrModelRevision}`}
          currentModel={bailianModel}
          selectedProtocol={bailianProtocol}
        />
      )}
      {unifiedBailian &&
        (bailianProtocol === 'dashscope-realtime' ||
          (bailianProtocol === 'auto' && bailianModelSupportsVocabulary(bailianModel))) && (
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
            <div
              style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}
            >
              {t('settings.providers.bailianVocabularyIdNote')}
            </div>
          </>
        )}
      {providerType === 'elevenlabs' && (
        <div
          role="note"
          style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}
        >
          {t('settings.providers.elevenLabsUploadNotice')}
        </div>
      )}
      {providerType === 'zenmux' && (
        <div
          role="note"
          style={{ marginTop: 2, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6 }}
        >
          {t('settings.providers.zenmuxVocabularyNote')}
        </div>
      )}
      {(providerType === 'openai-compatible' || providerType === 'zenmux') && (
        <AsrAdvancedOptions provider={channelId} onUserMutation={onUserMutation} />
      )}
      {/* Unified Bailian "fetch models" writes only model, never overwriting the user-chosen region or workspace endpoint. */}
      <ProviderTools
        kind="asr"
        disabled={Object.values(blockedFields).some(Boolean)}
        modelAccount="asr.model"
        provider={channelId}
        onModelSelected={() => setAsrModelRevision((v) => v + 1)}
        onTested={onTested}
        onUserMutation={onUserMutation}
      />
    </>
  );
}

// ASR advanced options: shown for the openai-compatible and zenmux presets.
// openai-compatible exposes verbose_json / chunk duration (other named vendors keep hardcoded
// behavior); zenmux exposes the enable_itn (number normalization) toggle — verbose_json /
// chunking are meaningless for it.
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
    verboseJson?: boolean;
    chunkDurationMs?: number | null;
    enableItn?: boolean;
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

// Under unified "Alibaba Cloud Bailian", decide the protocol by model name (kept in sync with
// the backend coordinator::resolve_effective_asr_provider): qwen3-asr-flash-realtime*,
// fun-asr-realtime*, and fun-asr-flash-8k-realtime* are realtime models; fun-asr-flash-2026-06-15
// and qwen-audio-3.0-asr-flash are "record then transcribe" (synchronous).
function bailianModelProtocol(model: string): 'realtime' | 'sync' | 'async' {
  const m = model.trim();
  if (!m || m.includes('realtime') || m === 'qwen-audio-3.0-asr-flash-streaming') return 'realtime';
  // qwen3-asr-flash-filetrans only accepts public URLs and is not supported yet (the backend
  // protocol_for_model rejects it explicitly), so the frontend no longer classifies it as async.
  if (
    m === 'fun-asr' ||
    (m.startsWith('fun-asr-') && !m.startsWith('fun-asr-flash')) ||
    m.startsWith('paraformer')
  )
    return 'async';
  // The rest (fun-asr-flash-*, qwen3-asr-flash, qwen-audio-3.0-asr-flash) are synchronous
  // record-file models.
  return 'sync';
}

// qwen-audio-3.0-asr-flash officially supports hotwords, but the batch protocol does not write
// that setting into the request body yet; until the backend supports it, do not show a hotword
// input that would have no effect.
function bailianModelSupportsVocabulary(model: string): boolean {
  const m = model.trim();
  return (
    !m ||
    m === 'qwen-audio-3.0-asr-flash-streaming' ||
    m.startsWith('fun-asr-realtime') ||
    m.startsWith('paraformer-realtime') ||
    m.startsWith('sensevoice-realtime')
  );
}

// One-line protocol hint under the model field, resolving "the three model types look
// identical" — tells the user whether the current model is realtime or record-file and how the
// behavior differs. Re-reads asr.model on asrModelRevision (model fetch/selection) and mount.
function BailianProtocolHint({
  currentModel,
  selectedProtocol,
}: {
  currentModel: string;
  selectedProtocol: BailianProtocol;
}) {
  const { t } = useTranslation();
  const protocol =
    selectedProtocol === 'auto'
      ? bailianModelProtocol(currentModel)
      : selectedProtocol === 'async-transcription'
        ? 'async'
        : selectedProtocol.endsWith('realtime')
          ? 'realtime'
          : 'sync';
  const hint =
    protocol === 'realtime'
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

/**
 * OrcaRouter exposes a large, changing catalog. Keep its own router models at
 * the top, then sort the remaining vendor/model ids for predictable scanning.
 */
export function prioritizeOrcaRouterModels(models: string[]): string[] {
  return [...models].sort((left, right) => {
    const leftOwn = left.startsWith('orcarouter/');
    const rightOwn = right.startsWith('orcarouter/');
    if (leftOwn !== rightOwn) return leftOwn ? -1 : 1;
    return left.localeCompare(right);
  });
}

function CatalogModelField({
  kind,
  provider,
  baseUrl,
  defaultModel,
  trailing,
  onUserMutation,
  onBlockedChange,
}: {
  kind: 'llm' | 'asr';
  provider: string;
  baseUrl: string;
  defaultModel: string;
  trailing?: ReactNode;
  onUserMutation?: () => void;
  onBlockedChange?: (account: string, blocked: boolean) => void;
}) {
  const { t } = useTranslation();
  const baseLayoutStack = useLayoutStack();
  const conservative = useConservativeLayout();
  const layoutStack = conservative || baseLayoutStack;
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [status, setStatus] = useState<ProviderToolStatus>('loading');
  const [message, setMessage] = useState(t('settings.providers.loadingModels'));
  const requestRef = useRef(0);
  const endpointAccount = kind === 'llm' ? 'ark.endpoint' : 'asr.endpoint';
  const modelAccount = kind === 'llm' ? 'ark.model_id' : 'asr.model';

  useEffect(() => {
    onBlockedChange?.(modelAccount, status !== 'success');
  }, [modelAccount, status, onBlockedChange]);

  const loadModels = async (initialize: boolean) => {
    const requestId = ++requestRef.current;
    setStatus('loading');
    setMessage(t('settings.providers.loadingModels'));
    try {
      // A newly created/migrated channel may not have received its preset yet.
      // Fill only an empty endpoint here; an explicit endpoint edit remains
      // respected, matching the behavior of the other named providers.
      if (initialize) {
        const endpoint = await readCredential(endpointAccount, provider);
        if (!endpoint?.trim()) {
          await setCredential(endpointAccount, baseUrl, provider);
        }
      }
      const [savedModel, result] = await Promise.all([
        readCredential(modelAccount, provider),
        listProviderModels(kind, provider, 'orcarouter'),
      ]);
      if (requestId !== requestRef.current) return;
      const nextModels = prioritizeOrcaRouterModels(result.models);
      setModels(nextModels);
      if (nextModels.length === 0) {
        setSelectedModel('');
        setStatus('empty');
        setMessage(t('settings.providers.modelsEmpty'));
        return;
      }

      const current = savedModel?.trim() ?? '';
      const nextModel = nextModels.includes(current)
        ? current
        : nextModels.includes(defaultModel)
          ? defaultModel
          : nextModels[0];
      if (nextModel !== current) {
        await setCredential(modelAccount, nextModel, provider);
      }
      if (requestId !== requestRef.current) return;
      setSelectedModel(nextModel);
      setStatus('success');
      setMessage(t('settings.providers.modelsLoaded', { count: nextModels.length }));
    } catch (error) {
      if (requestId !== requestRef.current) return;
      setModels([]);
      setStatus('error');
      setMessage(providerErrorMessage(error, t));
    }
  };

  useEffect(() => {
    void loadModels(true);
    return () => {
      requestRef.current += 1;
    };
    // The channel id defines the credential scope; changing it must reload the catalog.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider]);

  const applyModel = async (model: string) => {
    onUserMutation?.();
    setStatus('loading');
    setMessage(t('common.saving'));
    try {
      await setCredential(modelAccount, model, provider);
      setSelectedModel(model);
      setStatus('success');
      setMessage(t('settings.providers.modelSaved', { model }));
      emitSaved('saved', t('common.saved'));
    } catch (error) {
      setStatus('error');
      setMessage(providerErrorMessage(error, t));
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  return (
    <SettingRow label={t('settings.providers.modelLabel')}>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
          width: '100%',
          maxWidth: layoutStack ? '100%' : 420,
        }}
      >
        <div
          style={{
            display: 'flex',
            gap: 6,
            alignItems: 'center',
            width: '100%',
            flexWrap: layoutStack ? 'wrap' : 'nowrap',
          }}
        >
          <SelectLite
            value={selectedModel}
            onChange={(model) => void applyModel(model)}
            options={models.map((model) => ({ value: model, label: model }))}
            placeholder={
              status === 'loading'
                ? t('settings.providers.loadingModels')
                : t('settings.providers.selectModel')
            }
            disabled={status === 'loading' || models.length === 0}
            searchable
            searchPlaceholder={t('settings.providers.searchModels')}
            emptyMessage={t('settings.providers.noMatchingModels')}
            ariaLabel={t('settings.providers.selectModel')}
            style={{
              flex: layoutStack ? '1 1 100%' : 1,
              width: '100%',
              minWidth: 0,
              maxWidth: '100%',
              fontFamily: 'var(--ol-font-mono)',
            }}
          />
          <button
            onClick={() => {
              onUserMutation?.();
              void loadModels(false);
            }}
            title={t('common.refresh')}
            aria-label={t('common.refresh')}
            style={iconBtnStyle}
            disabled={status === 'loading'}
          >
            <Icon
              name="refresh"
              size={13}
              className={status === 'loading' ? 'ol-loading-spinner' : undefined}
            />
          </button>
          {trailing}
        </div>
        <span
          style={{
            fontSize: 11,
            color:
              status === 'error'
                ? 'var(--ol-warn)'
                : status === 'success'
                  ? 'var(--ol-ok)'
                  : 'var(--ol-ink-4)',
            lineHeight: 1.4,
          }}
        >
          {message}
        </span>
        <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', lineHeight: 1.4 }}>
          {t(
            kind === 'asr'
              ? 'settings.providers.orcarouterAsrCatalogHint'
              : 'settings.providers.orcarouterCatalogHint',
          )}
        </span>
      </div>
    </SettingRow>
  );
}

function ProviderTools({
  kind,
  modelAccount,
  provider,
  onModelSelected,
  onTested,
  onUserMutation,
  modelsUrl,
  showFetchModels = true,
  disabled = false,
  compact = false,
}: {
  kind: 'llm' | 'asr' | 'omni';
  modelAccount: string;
  provider?: string;
  onModelSelected: () => void;
  onTested?: () => void;
  onUserMutation?: () => void;
  modelsUrl?: string;
  showFetchModels?: boolean;
  disabled?: boolean;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [models, setModels] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState('');
  const [status, setStatus] = useState<ProviderToolStatus>('idle');
  const [message, setMessage] = useState('');
  const [operation, setOperation] = useState<'validate' | 'models'>('validate');

  const setResult = (next: ProviderToolStatus, nextMessage: string) => {
    if (!mounted.current) return;
    setStatus(next);
    setMessage(nextMessage);
  };

  // Persist the test result onto the channel (the card shows latency or an error highlight from
  // it). Failure does not interrupt the flow: the test already shows its conclusion next to the
  // button; a failed record only means one less history line on the card.
  const persistTest = async (ok: boolean, latencyMs: number | null, message: string | null) => {
    // Omni is not channelized (its own namespace), so there is no channel card to record into.
    if (!mounted.current || !provider || kind === 'omni') return;
    try {
      await recordChannelTest(kind, provider, ok, latencyMs, message);
      onTested?.();
    } catch (error) {
      console.error('[settings] failed to record channel test', error);
    }
  };

  const validate = async () => {
    if (disabled) return;
    onUserMutation?.();
    setOperation('validate');
    setResult('loading', t('settings.providers.validating'));
    const started = performance.now();
    try {
      const result = await validateProviderCredentials(kind, provider);
      const latency = Math.round(performance.now() - started);
      setResult(
        result.ok ? 'success' : 'error',
        t(
          result.ok
            ? kind === 'omni'
              ? 'settings.providers.validateSuccessOmni'
              : 'settings.providers.validateSuccess'
            : 'settings.providers.validateFailed',
        ),
      );
      await persistTest(result.ok, result.ok ? latency : null, result.ok ? null : 'validateFailed');
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (
        (kind === 'llm' && message === 'llmModelMissing') ||
        (kind === 'asr' && message === 'asrModelMissing')
      ) {
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
    if (disabled) return;
    onUserMutation?.();
    setOperation('models');
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
    if (disabled) return;
    onUserMutation?.();
    setOperation('models');
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

  const resultMessage = message && (
    <div
      className="ol-provider-result"
      key={`${operation}:${status}`}
      data-status={status}
      role="status"
      style={{
        fontSize: 12,
        color:
          status === 'error'
            ? 'var(--ol-warn)'
            : status === 'success'
              ? 'var(--ol-ok)'
              : 'var(--ol-ink-3)',
        lineHeight: 1.65,
        overflowWrap: 'anywhere',
        marginTop: 10,
      }}
    >
      <Icon
        name={status === 'success' ? 'check' : status === 'loading' ? 'refresh' : 'info'}
        size={15}
        className={status === 'loading' ? 'ol-loading-spinner' : undefined}
      />
      <span>{message}</span>
    </div>
  );

  if (compact) {
    return (
      <section className="ol-omni-tools" aria-label={t('settings.channels.validationTitle')}>
        <div className="ol-omni-tools-actions">
          {showFetchModels && (
            <button
              className="ol-channel-fetch-models"
              type="button"
              onClick={modelsUrl ? () => void openExternal(modelsUrl) : loadModels}
              style={miniBtnStyle}
              disabled={disabled || status === 'loading'}
            >
              <Icon
                name={modelsUrl ? 'external' : 'refresh'}
                size={14}
                className={
                  status === 'loading' && operation === 'models' ? 'ol-loading-spinner' : undefined
                }
              />
              {modelsUrl
                ? t('settings.providers.viewModels')
                : status === 'loading' && operation === 'models'
                  ? t('settings.providers.loadingModels')
                  : t('settings.providers.fetchModels')}
            </button>
          )}
          {!modelsUrl && models.length > 0 && (
            <SelectLite
              value={selectedModel}
              onChange={applyModel}
              disabled={disabled || status === 'loading'}
              options={models.map((model) => ({ value: model, label: model }))}
              placeholder={t('settings.providers.selectModel')}
              ariaLabel={t('settings.providers.selectModel')}
              style={{ flex: 1, minWidth: 140, maxWidth: '100%', height: 34 }}
            />
          )}
          <button
            className="ol-channel-verify"
            type="button"
            onClick={validate}
            style={{ ...miniBtnStyle, color: 'var(--ol-blue)', borderColor: 'var(--ol-blue)' }}
            disabled={disabled || status === 'loading'}
          >
            <Icon
              name={status === 'loading' && operation === 'validate' ? 'refresh' : 'play'}
              size={13}
              className={
                status === 'loading' && operation === 'validate' ? 'ol-loading-spinner' : undefined
              }
            />
            {status === 'loading' && operation === 'validate'
              ? t('settings.channels.verifying')
              : t('settings.channels.verify')}
          </button>
        </div>
        <p className="ol-omni-tools-hint">{t('settings.channels.validationHintOmni')}</p>
        {resultMessage}
      </section>
    );
  }

  return (
    <>
      {showFetchModels && (
        <ChannelFormRow label={t('settings.channels.availableModels')}>
          <div
            className="ol-channel-model-tools"
            data-populated={models.length > 0 ? 'true' : undefined}
            style={{
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'flex-start',
              gap: 10,
              minWidth: 0,
            }}
          >
            <button
              className="ol-channel-fetch-models"
              type="button"
              onClick={modelsUrl ? () => void openExternal(modelsUrl) : loadModels}
              style={miniBtnStyle}
              disabled={disabled || status === 'loading'}
            >
              <Icon
                name={modelsUrl ? 'external' : 'refresh'}
                size={14}
                className={
                  status === 'loading' && operation === 'models' ? 'ol-loading-spinner' : undefined
                }
              />
              {modelsUrl
                ? t('settings.providers.viewModels')
                : status === 'loading' && operation === 'models'
                  ? t('settings.providers.loadingModels')
                  : t('settings.providers.fetchModels')}
            </button>
            {!modelsUrl && models.length > 0 && (
              <SelectLite
                value={selectedModel}
                onChange={applyModel}
                disabled={disabled || status === 'loading'}
                options={models.map((model) => ({ value: model, label: model }))}
                placeholder={t('settings.providers.selectModel')}
                ariaLabel={t('settings.providers.selectModel')}
                style={{ width: '100%', minWidth: 0, height: 38 }}
              />
            )}
          </div>
          {operation === 'models' && resultMessage}
        </ChannelFormRow>
      )}
      <section className="ol-channel-validation" style={channelSectionStyle}>
        <div className="ol-channel-validation-heading">
          <ChannelSectionHeading
            icon="bolt"
            title={t('settings.channels.validationTitle')}
            description={t(
              kind === 'omni'
                ? 'settings.channels.validationHintOmni'
                : 'settings.channels.validationHint',
            )}
          />
          <button
            className="ol-channel-verify"
            type="button"
            onClick={validate}
            style={{
              ...miniBtnStyle,
              marginTop: 12,
              color: 'var(--ol-blue)',
              borderColor: 'var(--ol-blue)',
            }}
            disabled={disabled || status === 'loading'}
          >
            <Icon
              name={status === 'loading' && operation === 'validate' ? 'refresh' : 'play'}
              size={13}
              className={
                status === 'loading' && operation === 'validate' ? 'ol-loading-spinner' : undefined
              }
            />
            {status === 'loading' && operation === 'validate'
              ? t('settings.channels.verifying')
              : t('settings.channels.verify')}
          </button>
        </div>
        {operation === 'validate' && resultMessage}
      </section>
    </>
  );
}

function providerErrorMessage(error: unknown, t: ReturnType<typeof useTranslation>['t']): string {
  const message = error instanceof Error ? error.message : String(error);
  for (const code of [
    'volcengineServiceInvalid',
    'llmRequestFormatInvalid',
    'llmThinkingModeInvalid',
    'llmTokenLimitInvalid',
    'llmThinkingBudgetInvalid',
    'llmResponseIncomplete',
    'llmStreamError',
    'llmProtocolHeaderConflict',
  ]) {
    if (message.includes(code)) return t(`settings.providers.${code}`);
  }
  if (message.startsWith('providerHttpStatus:')) {
    return t('settings.providers.providerHttpStatus', { status: message.split(':')[1] || '?' });
  }
  if (message === 'endpointMustUseHttps') return t('settings.providers.endpointMustUseHttps');
  if (message === 'endpointInvalid') return t('settings.providers.endpointInvalid');
  if (message === 'bailianEndpointSchemeInvalid')
    return t('settings.providers.bailianEndpointSchemeInvalid');
  if (message === 'qwen3EndpointSchemeInvalid')
    return t('settings.providers.qwen3EndpointSchemeInvalid');
  if (message === 'providerResponseTooLarge') return t('settings.providers.responseTooLarge');
  if (message === 'asrInvalidJson') return t('settings.providers.asrInvalidJson');
  if (message === 'asrMissingTextField') return t('settings.providers.asrMissingTextField');
  if (message === 'providerNetworkError') return t('common.networkError');
  if (message === 'providerReadResponseFailed' || message === 'providerClientInitFailed')
    return t('common.operationFailed');
  if (message === 'providerRequestTimeout') return t('settings.providers.requestTimeout');
  if (message === 'volcengineAppIdMissing') return t('settings.providers.volcengineAppIdMissing');
  if (message === 'volcengineAccessTokenMissing')
    return t('settings.providers.volcengineAccessTokenMissing');
  if (message === 'volcengineApiKeyMissing') return t('settings.providers.apiKeyMissing');
  // Volcano handshake rejection/rate-limit errors carry a status code and scenario text;
  // passing them through is more useful than a generic "operation failed".
  if (message.includes('凭据被拒') || message.includes('被限流')) return message;
  if (message.includes('API Key')) return t('settings.providers.apiKeyMissing');
  if (message.includes('Endpoint')) return t('settings.providers.endpointMissing');
  if (message.includes('timeout') || message.includes('超时'))
    return t('settings.providers.requestTimeout');
  if (
    message.startsWith('task failed:') ||
    message.startsWith('connection failed:') ||
    message.startsWith('send failed:')
  ) {
    return message;
  }
  return t('common.operationFailed');
}

type CredentialFieldStatus =
  'idle' | 'saving' | 'saved' | 'readError' | 'saveError' | 'copied' | 'copyError';

interface CredentialFieldProps {
  onBlockedChange?: (account: string, blocked: boolean) => void;
  label: string;
  account: string;
  provider?: string;
  placeholder?: string;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
  trailing?: ReactNode;
  onValueChange?: (value: string) => void;
  /** Fires only when the user directly edits the field; initial read, copy, and show/hide do not trigger it. */
  onUserMutation?: () => void;
  /** When provided, renders a dropdown (preset selection) instead of an input; appends the current value as a custom item if not in the presets. */
  options?: SelectOption[];
  /** Endpoint presets and manual input share one field and one save queue. */
  endpointPresets?: { label: string; options: SelectOption[] };
}

function CredentialField({
  label,
  account,
  provider,
  placeholder,
  mono,
  mask,
  defaultValue,
  trailing,
  onValueChange,
  onUserMutation,
  options,
  endpointPresets,
  onBlockedChange,
}: CredentialFieldProps) {
  const fieldId = useId();
  const { t } = useTranslation();
  // Let the host ChannelModal settle this field's pending debounced writes before close/switch (#1044 semantics).
  const form = useContext(ProviderFormContext);
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<CredentialFieldStatus>('idle');
  // The "Custom model…" escape hatch in the preset dropdown: selecting it switches back to an
  // input so any model name the backend supports can be typed manually.
  const [customModelMode, setCustomModelMode] = useState(false);
  useEffect(() => {
    onBlockedChange?.(
      account,
      !loaded || dirty || status === 'saving' || status === 'readError' || status === 'saveError',
    );
    form?.track(
      account,
      !loaded || dirty || status === 'saving' || status === 'readError' || status === 'saveError',
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps -- form?.track and onBlockedChange are both stable refs
  }, [account, loaded, dirty, status, onBlockedChange]);

  const debounceRef = useRef<number | null>(null);
  const statusRef = useRef<number | null>(null);
  const mountedRef = useRef(true);
  const editRevision = useRef(0);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());
  const lastWriteOk = useRef(true);
  const flushRef = useRef<() => Promise<boolean>>(() => Promise.resolve(true));
  // Flush before leaving: push debounced edits not yet sent and await in-flight writes; returning false blocks the close.
  useEffect(
    () => form?.register(account, () => flushRef.current()),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- register is stable; account change remounts
    [account],
  );
  const markMutation = () => {
    editRevision.current += 1;
    onUserMutation?.();
  };

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
      .then((v) => {
        if (cancelled) return;
        setValue(v ?? '');
        onValueChange?.(v ?? '');
        setLoaded(true);
      })
      .catch((error) => {
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

  // Design: except readError (a persistent error that stays next to the input to mark the field
  // unavailable), all saving / saved / saveError / copied / copyError states go to the top-right
  // SavedToast. Inline text was too cramped and inconsistent with other pages' toast style.
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
    const revision = editRevision.current;
    setStatus('saving');
    emitSaved('saving', t('common.saving'));
    try {
      // Write in edit order; an older request finishing must not mark the newer value as saved.
      const write = saveQueue.current
        .catch(() => undefined)
        .then(() => setCredential(account, v, provider));
      saveQueue.current = write;
      await write;
      if (!mountedRef.current || revision !== editRevision.current) return;
      lastWriteOk.current = true;
      setDirty(false);
      showTemporaryStatus('saved');
    } catch (error) {
      if (!mountedRef.current || revision !== editRevision.current) return;
      lastWriteOk.current = false;
      console.error('[settings] failed to save credential', account, error);
      showTemporaryStatus('saveError');
    }
  };
  flushRef.current = async () => {
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    if (loaded && dirty) await save(value, true);
    await saveQueue.current.catch(() => undefined);
    return lastWriteOk.current;
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    markMutation();
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
    markMutation();
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
  const disabled = !loaded || form?.leaving;
  const showInsecureEndpointWarning =
    (account === 'ark.endpoint' || account === 'asr.endpoint' || account === 'omni.endpoint') &&
    value.trim().toLowerCase().startsWith('http://');

  const presetValue =
    endpointPresets?.options.find((option) => {
      return matchesEndpointPreset(value || defaultValue || '', option.value);
    })?.value || '';

  return (
    <>
      {endpointPresets && (
        <ChannelFormRow label={endpointPresets.label}>
          <SelectLite
            value={presetValue}
            options={endpointPresets.options}
            placeholder={loaded ? t('settings.providers.presets.custom') : t('common.loading')}
            disabled={disabled || status === 'readError'}
            ariaLabel={endpointPresets.label}
            onChange={(next) => {
              if (debounceRef.current) {
                clearTimeout(debounceRef.current);
                debounceRef.current = null;
              }
              markMutation();
              setValue(next);
              onValueChange?.(next);
              setDirty(true);
              void save(next, true);
            }}
            style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
          />
        </ChannelFormRow>
      )}
      <ChannelFormRow label={label} htmlFor={options && !customModelMode ? undefined : fieldId}>
        <div
          style={{ display: 'flex', flexDirection: 'column', gap: 5, width: '100%', minWidth: 0 }}
        >
          <div
            style={{
              display: 'flex',
              gap: 6,
              alignItems: 'center',
              width: '100%',
              flexWrap: 'nowrap',
            }}
          >
            {options && !customModelMode ? (
              <SelectLite
                value={value}
                onChange={(v) => {
                  // "Custom model…" escape hatch: switch back to the input to type any model name.
                  if (v === CUSTOM_MODEL_OPTION_VALUE) {
                    setCustomModelMode(true);
                    return;
                  }
                  markMutation();
                  setValue(v);
                  onValueChange?.(v);
                  if (!loaded) return;
                  setDirty(true);
                  void save(v, true);
                }}
                options={[
                  ...(value && !options.some((o) => o.value === value)
                    ? [{ value, label: value }]
                    : []),
                  ...options,
                  {
                    value: CUSTOM_MODEL_OPTION_VALUE,
                    label: t('settings.providers.customModelLabel', 'Custom model…'),
                  },
                ]}
                placeholder={loaded ? placeholder : t('common.loading')}
                disabled={disabled}
                ariaLabel={label}
                style={{
                  flex: 1,
                  height: 38,
                  minWidth: 0,
                  maxWidth: '100%',
                  fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit',
                }}
              />
            ) : (
              <input
                id={fieldId}
                type={inputType}
                value={value}
                placeholder={loaded ? placeholder : t('common.loading')}
                onChange={handleChange}
                onBlur={onBlur}
                disabled={disabled}
                readOnly={!!endpointPresets && !!presetValue}
                style={{
                  ...inputStyle,
                  flex: 1,
                  height: 38,
                  minWidth: 0,
                  maxWidth: '100%',
                  fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit',
                }}
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
              <button
                onClick={fillDefault}
                title={t('settings.providers.fillDefault')}
                style={iconBtnStyle}
                disabled={!loaded}
              >
                <Icon name="check" size={13} />
              </button>
            )}
            {mask && (
              <button
                onClick={() => setRevealed((r) => !r)}
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
            {/* readError is a persistent "field unreadable" error and stays in place to tell the
              user the field is unavailable; other transient states (saving / saved / saveError /
              copied / copyError) go through emitSaved to the unified top-right toast instead of
              inline placeholders. */}
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
          {trailing && (
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, marginTop: 6 }}>
              {trailing}
            </div>
          )}
          {showInsecureEndpointWarning && (
            <span style={{ fontSize: 11, color: 'var(--ol-warn)', lineHeight: 1.45 }}>
              {t('settings.providers.endpointHttpWarning')}
            </span>
          )}
        </div>
      </ChannelFormRow>
    </>
  );
}

const miniBtnStyle: CSSProperties = {
  height: 32,
  padding: '0 12px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-control-solid)',
  boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
  color: 'var(--ol-ink-2)',
  cursor: 'pointer',
  flexShrink: 0,
  fontSize: 12.5,
  fontWeight: 500,
  letterSpacing: '0.01em',
  transition:
    'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), box-shadow 0.16s var(--ol-motion-quick)',
};

const iconBtnStyle: CSSProperties = {
  width: 32,
  height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-control-solid)',
  boxShadow: '0 1px 2px rgba(0,0,0,0.04)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  color: 'var(--ol-ink-3)',
  cursor: 'pointer',
  flexShrink: 0,
  transition:
    'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), color 0.16s var(--ol-motion-quick), transform 0.12s var(--ol-motion-quick)',
};

/** Compact configuration for the active multimodal pipeline. */
export function OmniChannelSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [descriptors, setDescriptors] = useState<ProviderDescriptor[]>([]);
  const [omniProvider, setOmniProvider] = useState('custom');
  const [committedOmniProvider, setCommittedOmniProvider] = useState('custom');
  const omniSwitchSeqRef = useRef(0);
  const [omniModelRevision, setOmniModelRevision] = useState(0);
  const formRef = useRef<HTMLDivElement>(null);
  useContentMotion(formRef, committedOmniProvider);

  useEffect(() => {
    void listProviderDescriptors('omni')
      .then(setDescriptors)
      .catch((error) =>
        console.error('[settings] failed to load omni provider descriptors', error),
      );
  }, []);

  const omniPresets = useMemo(
    () =>
      descriptors.map((descriptor) => ({
        id: descriptor.providerType,
        nameKey: descriptor.labelKey,
        baseUrl: descriptor.defaultEndpoint ?? '',
        modelPlaceholder: descriptor.defaultModel ?? '',
      })),
    [descriptors],
  );

  useEffect(() => {
    if (!prefs) return;
    const knownOmni = omniPresets.find((x) => x.id === prefs.activeOmniProvider);
    const omniId = knownOmni ? knownOmni.id : 'custom';
    setOmniProvider(omniId);
    setCommittedOmniProvider(omniId);
  }, [prefs, omniPresets]);

  // Same semantics as the LLM card: controlled dropdown gives immediate feedback + committed
  // controls CredentialField remount + a seq guard prevents stale overwrites; only credentials
  // land in omni.* slots.
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
      const preset = omniPresets.find((p) => p.id === id);
      // Switching to a non-custom preset force-overwrites endpoint/model defaults (same as the
      // LLM card), so the switch truly takes effect and no old vendor slot values linger.
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

  const omniPreset = omniPresets.find((p) => p.id === committedOmniProvider);

  return (
    <Card className="ol-omni-settings" padding={16}>
      <div ref={formRef}>
        <div style={{ marginBottom: 4 }}>
          <SectionTitle>{t('settings.providers.omniTitle')}</SectionTitle>
        </div>
        <div className="ol-omni-primary-fields">
          <ChannelFormRow label={t('settings.providers.providerLabel')}>
            <SelectLite
              value={omniProvider}
              onChange={(next) => onOmniProviderChange(next)}
              options={omniPresets.map((p) => ({
                value: p.id,
                label: t(`settings.providers.presets.${p.nameKey}`),
              }))}
              ariaLabel={t('settings.providers.providerLabel')}
              style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
            />
          </ChannelFormRow>
          <CredentialField
            key={`${committedOmniProvider}:model:${omniModelRevision}`}
            label={t('settings.providers.modelLabel')}
            account="omni.model"
            placeholder={omniPreset?.modelPlaceholder || 'model-name'}
            mono
          />
        </div>
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
          <div className="ol-omni-advanced-fields">
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
          </div>
        )}
        <ProviderTools
          key={`omni:${committedOmniProvider}`}
          kind="omni"
          modelAccount="omni.model"
          onModelSelected={() => setOmniModelRevision((v) => v + 1)}
          compact
        />
      </div>
    </Card>
  );
}
