// Settings.tsx — ported verbatim from design_handoff_openless/pages.jsx::Settings.
// Internal sub-sections (Recording / Providers / Shortcuts / Permissions / Language / About)
// keep their inline-style literals 1:1 with the source JSX.

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { isDialogStatus, UpdateDialog, useAutoUpdate } from '../components/AutoUpdate';
import { APP_VERSION_LABEL } from '../lib/appVersion';
import { isHotkeyModeMigrationNoticeActive } from '../lib/hotkeyMigration';
import { getHotkeyStartStopLabel, getHotkeyTriggerLabel } from '../lib/hotkey';
import { DEFAULT_LLM_PRESET_ID, LLM_PRESETS, type LlmPresetId } from '../lib/llmPresets';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  openExternal,
  openSystemSettings,
  listProviderModels,
  readCredential,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  setActiveAsrProvider,
  setActiveLlmProvider,
  setCredential,
  validateProviderCredentials,
} from '../lib/ipc';
import type {
  HotkeyCapability,
  HotkeyMode,
  HotkeyStatus,
  HotkeyTrigger,
  PermissionStatus,
} from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import i18n, {
  FOLLOW_SYSTEM,
  getLocalePreference,
  setLocalePreference,
  type SupportedLocale,
} from '../i18n';
import { Btn, Card, PageHeader, Pill } from './_atoms';

interface SettingsProps {
  embedded?: boolean;
  initialSection?: SettingsSectionId;
}

export type SettingsSectionId = 'recording' | 'providers' | 'shortcuts' | 'permissions' | 'language' | 'about';

const SECTION_ORDER: SettingsSectionId[] = ['recording', 'providers', 'shortcuts', 'permissions', 'language', 'about'];

export function Settings({ embedded = false, initialSection = 'recording' }: SettingsProps) {
  const { t } = useTranslation();
  const [section, setSection] = useState<SettingsSectionId>(initialSection);

  useEffect(() => {
    setSection(initialSection);
  }, [initialSection]);

  return (
    <>
      {!embedded && (
        <PageHeader
          kicker={t('settings.kicker')}
          title={t('settings.title')}
          desc={t('settings.desc')}
        />
      )}
      {/* embedded（在 SettingsModal 里）模式下：mini-sidebar 固定，仅右栏 scroll。
          外层 flex:1 minHeight:0 让 grid 拿到确定高度；gridTemplateRows: minmax(0, 1fr)
          强制行高等于容器高度，否则 grid 默认 auto rows 会跟内容长，右栏 overflow:auto
          就退化成"没东西需要 scroll"，于是大家照旧一起飘。 */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: embedded ? '120px 1fr' : '160px 1fr',
          gap: 18,
          ...(embedded ? { flex: 1, minHeight: 0, gridTemplateRows: 'minmax(0, 1fr)' } : {}),
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {SECTION_ORDER.map(s => (
            <button
              key={s}
              onClick={() => setSection(s)}
              style={{
                padding: '8px 12px', textAlign: 'left',
                fontSize: 13, color: section === s ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                background: section === s ? 'rgba(0,0,0,0.04)' : 'transparent',
                border: 0, borderRadius: 8, fontFamily: 'inherit', fontWeight: section === s ? 600 : 500,
                cursor: 'default',
                transition: 'background 0.12s ease-out, color 0.12s ease-out',
              }}
            >
              {t(`settings.sections.${s}`)}
            </button>
          ))}
        </div>
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: 12,
            ...(embedded ? { minHeight: 0, overflow: 'auto', paddingRight: 4 } : {}),
          }}
        >
          {section === 'recording' && <RecordingSection />}
          {section === 'providers' && <ProvidersSection />}
          {section === 'shortcuts' && <ShortcutsSection />}
          {section === 'permissions' && <PermissionsSection />}
          {section === 'language' && <LanguageSection />}
          {section === 'about' && <AboutSection />}
        </div>
      </div>
    </>
  );
}

interface SettingRowProps {
  label: string;
  desc?: string;
  children: ReactNode;
}

function SettingRow({ label, desc, children }: SettingRowProps) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: 16, padding: '14px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
      <div>
        <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
        {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
      </div>
      <div style={{ display: 'flex', alignItems: 'flex-start' }}>{children}</div>
    </div>
  );
}

function RecordingSection() {
  const { t } = useTranslation();
  const { prefs, capability, updatePrefs: savePrefs } = useHotkeySettings();

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const onTriggerChange = (trigger: HotkeyTrigger) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, trigger } });
  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) =>
    savePrefs({ ...prefs, showCapsule });
  const onRestoreClipboardChange = (restoreClipboardAfterPaste: boolean) =>
    savePrefs({ ...prefs, restoreClipboardAfterPaste });

  const choices: Array<[HotkeyMode, string]> = [
    ['toggle', t('settings.recording.modeToggle')],
    ['hold', t('settings.recording.modeHold')],
  ];
  const hotkeyDesc = capability.requiresAccessibilityPermission
    ? t('settings.recording.hotkeyDescAcc')
    : t('settings.recording.hotkeyDescNoAcc');

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.recording.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{t('settings.recording.desc')}</div>
      {isHotkeyModeMigrationNoticeActive() && (
        <div
          style={{
            marginTop: 10,
            marginBottom: 8,
            padding: '12px 14px',
            borderRadius: 10,
            background: 'rgba(37,99,235,0.08)',
            border: '0.5px solid rgba(37,99,235,0.18)',
          }}
        >
          <div style={{ fontSize: 12.5, fontWeight: 600, color: 'var(--ol-blue)', marginBottom: 4 }}>
            {t('settings.recording.migrationNoticeTitle')}
          </div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', lineHeight: 1.55 }}>
            {t('settings.recording.migrationNoticeDesc')}
          </div>
        </div>
      )}
      <SettingRow label={t('settings.recording.hotkeyLabel')} desc={hotkeyDesc}>
        <select
          value={prefs.hotkey.trigger}
          onChange={e => onTriggerChange(e.target.value as HotkeyTrigger)}
          style={{
            ...inputStyle,
            maxWidth: 200,
            fontFamily: 'var(--ol-font-mono)',
          }}
        >
          {capability.availableTriggers.map(tr => (
            <option key={tr} value={tr}>{getHotkeyTriggerLabel(tr)}</option>
          ))}
        </select>
      </SettingRow>
      <SettingRow label={t('settings.recording.modeLabel')} desc={t('settings.recording.modeDesc')}>
        <div style={{ display: 'inline-flex', padding: 2, borderRadius: 8, background: 'rgba(0,0,0,0.05)' }}>
          {choices.map(([v, l]) => (
            <button
              key={v}
              onClick={() => onModeChange(v)}
              style={{
                padding: '5px 14px', fontSize: 12, fontWeight: 500,
                border: 0, borderRadius: 6, fontFamily: 'inherit',
                background: prefs.hotkey.mode === v ? '#fff' : 'transparent',
                color: prefs.hotkey.mode === v ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                boxShadow: prefs.hotkey.mode === v ? '0 1px 2px rgba(0,0,0,.08)' : 'none',
                cursor: 'default',
                transition: 'background 0.12s ease-out, color 0.12s ease-out, box-shadow 0.12s ease-out',
              }}
            >
              {l}
            </button>
          ))}
        </div>
      </SettingRow>
      <SettingRow label={t('settings.recording.capsuleLabel')} desc={t('settings.recording.capsuleDesc')}>
        <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
      </SettingRow>
      <SettingRow
        label={t('settings.recording.restoreClipboardLabel')}
        desc={t('settings.recording.restoreClipboardDesc')}
      >
        <Toggle on={prefs.restoreClipboardAfterPaste} onToggle={onRestoreClipboardChange} />
      </SettingRow>
      {capability.statusHint && (
        <div style={{ marginTop: 6, fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.5 }}>
          {capability.statusHint}
        </div>
      )}
    </Card>
  );
}

function Toggle({ on, onToggle }: { on: boolean; onToggle?: (next: boolean) => void }) {
  return (
    <button
      onClick={() => onToggle?.(!on)}
      style={{
        position: 'relative', width: 32, height: 18, borderRadius: 999, border: 0,
        background: on ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
        cursor: 'default',
        transition: 'background 0.15s ease-out',
      }}
    >
      <span
        style={{
          position: 'absolute', top: 2, left: on ? 16 : 2,
          width: 14, height: 14, borderRadius: 999, background: '#fff',
          boxShadow: '0 1px 2px rgba(0,0,0,.25)', transition: 'left .15s',
        }}
      />
    </button>
  );
}

const ASR_DEFAULT_RESOURCE_ID = 'volc.bigasr.sauc.duration';

// SiliconFlow ASR 暂未在后端实现（coordinator.rs 只路由 whisper / volcengine）。
// 在后端接入前不暴露给用户，避免选了之后必然失败。重新启用见 issue #58 的 follow-up。
const ASR_PRESETS = [
  { id: 'volcengine',  nameKey: 'asrVolcengine'  },
  { id: 'whisper',     nameKey: 'asrWhisper'     },
] as const;

type AsrPresetId = typeof ASR_PRESETS[number]['id'];

function ProvidersSection() {
  const { t } = useTranslation();
  const { prefs, updatePrefs } = useHotkeySettings();
  const [llmProvider, setLlmProvider] = useState<LlmPresetId>(DEFAULT_LLM_PRESET_ID);
  const [asrProvider, setAsrProvider] = useState<AsrPresetId>('volcengine');
  const [llmModelRevision, setLlmModelRevision] = useState(0);
  const [asrModelRevision, setAsrModelRevision] = useState(0);

  useEffect(() => {
    if (!prefs) return;
    const knownLlm = LLM_PRESETS.find(x => x.id === prefs.activeLlmProvider);
    setLlmProvider(knownLlm ? knownLlm.id : 'custom');
    const knownAsr = ASR_PRESETS.find(x => x.id === prefs.activeAsrProvider);
    setAsrProvider(knownAsr ? knownAsr.id : 'volcengine');
  }, [prefs]);

  const onLlmProviderChange = async (id: LlmPresetId) => {
    setLlmProvider(id);
    await setActiveLlmProvider(id);
    if (prefs) {
      const next = { ...prefs, activeLlmProvider: id };
      await updatePrefs(next);
    }
    const preset = LLM_PRESETS.find(p => p.id === id);
    if (preset?.baseUrl) {
      await setCredential('ark.endpoint', preset.baseUrl);
    }
  };

  const onAsrProviderChange = async (id: AsrPresetId) => {
    setAsrProvider(id);
    await setActiveAsrProvider(id);
    if (prefs) {
      const next = { ...prefs, activeAsrProvider: id };
      await updatePrefs(next);
    }
  };

  const preset = LLM_PRESETS.find(p => p.id === llmProvider) ?? LLM_PRESETS[LLM_PRESETS.length - 1];

  return (
    <>
      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.providers.llmTitle')}</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>
            {t('settings.providers.llmDesc')}
          </div>
        </div>
        <SettingRow label={t('settings.providers.providerLabel')} desc={t('settings.providers.llmProviderDesc')}>
          <select
            value={llmProvider}
            onChange={e => onLlmProviderChange(e.target.value as LlmPresetId)}
            style={{ ...inputStyle, maxWidth: 200 }}
          >
            {LLM_PRESETS.map(p => (
              <option key={p.id} value={p.id}>{t(`settings.providers.presets.${p.nameKey}`)}</option>
            ))}
          </select>
        </SettingRow>
        <CredentialField key={`${llmProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="ark.api_key" mono mask />
        <CredentialField key={`${llmProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="ark.endpoint"
          placeholder={preset.baseUrl || 'https://your-endpoint/v1'} />
        <CredentialField key={`${llmProvider}:model:${llmModelRevision}`} label={t('settings.providers.modelLabel')} account="ark.model_id"
          placeholder={preset.modelPlaceholder || 'model-name'} mono />
        <ProviderTools key={llmProvider} kind="llm" modelAccount="ark.model_id" onModelSelected={() => setLlmModelRevision(v => v + 1)} />
      </Card>

      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>{t('settings.providers.asrTitle')}</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>{t('settings.providers.asrDesc')}</div>
        </div>
        <SettingRow label={t('settings.providers.providerLabel')} desc={t('settings.providers.asrProviderDesc')}>
          <select
            value={asrProvider}
            onChange={e => onAsrProviderChange(e.target.value as AsrPresetId)}
            style={{ ...inputStyle, maxWidth: 200 }}
          >
            {ASR_PRESETS.map(p => (
              <option key={p.id} value={p.id}>{t(`settings.providers.presets.${p.nameKey}`)}</option>
            ))}
          </select>
        </SettingRow>
        {asrProvider === 'volcengine' ? (
          <>
            <CredentialField key={`${asrProvider}:app_key`} label={t('settings.providers.appIdLabel')} account="volcengine.app_key" mono mask />
            <CredentialField key={`${asrProvider}:access_key`} label={t('settings.providers.accessKeyLabel')} account="volcengine.access_key" mono mask />
            <CredentialField key={`${asrProvider}:resource_id`} label={t('settings.providers.resourceIdLabel')} account="volcengine.resource_id" mono
              placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
          </>
        ) : (
          <>
            <CredentialField key={`${asrProvider}:api_key`} label={t('settings.providers.apiKeyLabel')} account="asr.api_key" mono mask />
            <CredentialField key={`${asrProvider}:endpoint`} label={t('settings.providers.baseUrlLabel')} account="asr.endpoint"
              placeholder="https://api.openai.com/v1" defaultValue="https://api.openai.com/v1" />
            <CredentialField key={`${asrProvider}:model:${asrModelRevision}`} label={t('settings.providers.modelLabel')} account="asr.model"
              placeholder="whisper-1" />
            <ProviderTools kind="asr" modelAccount="asr.model" onModelSelected={() => setAsrModelRevision(v => v + 1)} />
          </>
        )}
      </Card>
    </>
  );
}

type ProviderToolStatus = 'idle' | 'loading' | 'success' | 'empty' | 'error';

function ProviderTools({ kind, modelAccount, onModelSelected }: { kind: 'llm' | 'asr'; modelAccount: string; onModelSelected: () => void }) {
  const { t } = useTranslation();
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
        result.ok ? (result.modelCount === 0 ? 'empty' : 'success') : 'error',
        result.modelCount === 0
          ? t('settings.providers.modelsEmpty')
          : t('settings.providers.validateSuccess', { count: result.modelCount }),
      );
    } catch (error) {
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
    <SettingRow label={t('settings.providers.toolsLabel')} desc={t('settings.providers.toolsDesc')}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, width: '100%', maxWidth: 420 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
          <button onClick={validate} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.validate')}</button>
          <button onClick={loadModels} style={miniBtnStyle} disabled={status === 'loading'}>{t('settings.providers.fetchModels')}</button>
          {models.length > 0 && (
            <select
              value={selectedModel}
              onChange={e => applyModel(e.target.value)}
              disabled={status === 'loading'}
              style={{ ...inputStyle, maxWidth: 220 }}
            >
              <option value="" disabled>{t('settings.providers.selectModel')}</option>
              {models.map(model => <option key={model} value={model}>{model}</option>)}
            </select>
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
}

function CredentialField({ label, account, placeholder, mono, mask, defaultValue }: CredentialFieldProps) {
  const { t } = useTranslation();
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState<CredentialFieldStatus>('idle');
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const statusRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
      if (statusRef.current) clearTimeout(statusRef.current);
    };
  }, []);

  const showTemporaryStatus = (next: CredentialFieldStatus) => {
    setStatus(next);
    if (statusRef.current) clearTimeout(statusRef.current);
    statusRef.current = window.setTimeout(() => setStatus('idle'), 1600);
  };

  const save = async (v: string) => {
    if (!loaded || !dirty) return;
    setStatus('saving');
    try {
      await setCredential(account, v);
      setDirty(false);
      showTemporaryStatus('saved');
    } catch (error) {
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
    debounceRef.current = setTimeout(() => save(v), 300);
  };

  const onBlur = () => {
    if (!loaded || !dirty) return;
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    save(value);
  };

  const fillDefault = async () => {
    if (!loaded || !defaultValue) return;
    setValue(defaultValue);
    setDirty(true);
    await save(defaultValue);
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

  return (
    <SettingRow label={label}>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', maxWidth: 420 }}>
        <input
          type={inputType}
          value={value}
          placeholder={loaded ? placeholder : t('common.loading')}
          onChange={handleChange}
          onBlur={onBlur}
          disabled={disabled}
          style={{ ...inputStyle, fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
        />
        {defaultValue && !value && loaded && (
          <button onClick={fillDefault} title={t('settings.providers.fillDefault')} style={iconBtnStyle} disabled={!loaded}>
            <Icon name="check" size={13} />
          </button>
        )}
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
        {status !== 'idle' && (
          <span
            style={{
              fontSize: 11,
              color: status.endsWith('Error') ? 'var(--ol-warn)' : 'var(--ol-ok)',
              whiteSpace: 'nowrap',
            }}
          >
            {status === 'saving'
              ? t('common.saving')
              : status === 'saved'
                ? t('common.saved')
                : status === 'copied'
                  ? t('common.copied')
                  : status === 'readError'
                    ? t('settings.providers.readFailed')
                    : t('common.operationFailed')}
          </span>
        )}
      </div>
    </SettingRow>
  );
}

const inputStyle: CSSProperties = {
  flex: 1, height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, fontSize: 12.5,
  fontFamily: 'inherit', outline: 'none',
  background: 'var(--ol-surface-2)',
  width: '100%', maxWidth: 360,
  transition: 'background 0.12s ease-out, border-color 0.12s ease-out',
};
const miniBtnStyle: CSSProperties = {
  height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  color: 'var(--ol-ink-2)', cursor: 'default', flexShrink: 0,
  fontSize: 12, fontWeight: 500,
  transition: 'background 0.12s ease-out, border-color 0.12s ease-out, color 0.12s ease-out',
};

const iconBtnStyle: CSSProperties = {
  width: 32, height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  color: 'var(--ol-ink-3)', cursor: 'default', flexShrink: 0,
  transition: 'background 0.12s ease-out, border-color 0.12s ease-out, color 0.12s ease-out',
};

function ShortcutsSection() {
  const { t } = useTranslation();
  const { hotkey, capability } = useHotkeySettings();

  if (!hotkey || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>{t('common.loading')}</div>
      </Card>
    );
  }

  const desc = capability.requiresAccessibilityPermission
    ? t('settings.shortcuts.descAcc')
    : t('settings.shortcuts.descNoAcc');
  const notSupported = t('settings.shortcuts.notSupported');
  const rows: Array<[string, string]> = [
    [t('settings.shortcuts.startStop'), getHotkeyStartStopLabel(hotkey)],
    [t('settings.shortcuts.cancel'), 'Esc'],
    [t('settings.shortcuts.confirm'), t('settings.shortcuts.confirmHint')],
    [t('settings.shortcuts.switchStyle'), capability.requiresAccessibilityPermission ? '⌘ ⇧ S' : notSupported],
    [t('settings.shortcuts.openApp'), capability.requiresAccessibilityPermission ? '⌘ ⇧ O' : notSupported],
  ];
  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.shortcuts.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{desc}</div>
      {rows.map(([k, v]) => (
        <SettingRow key={k} label={k}>
          <kbd style={{
            display: 'inline-flex', alignItems: 'center', gap: 4,
            padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
            borderRadius: 6, background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line-strong)',
            boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
            color: 'var(--ol-ink-2)',
          }}>{v}</kbd>
        </SettingRow>
      ))}
    </Card>
  );
}

function PermissionsSection() {
  const { t } = useTranslation();
  const [accessibility, setAccessibility] = useState<PermissionStatus | 'loading'>('loading');
  const [microphone, setMicrophone] = useState<PermissionStatus | 'loading'>('loading');
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const { capability } = useHotkeySettings();

  const refreshPermissions = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
  };

  const refreshHotkey = async () => {
    setHotkey(await getHotkeyStatus());
  };

  useEffect(() => {
    refreshPermissions();
    refreshHotkey();
    const hotkeyId = window.setInterval(refreshHotkey, 1000);
    // 麦克风检查会短暂打开输入流，避免每秒探测导致隐私指示器频繁闪烁。
    const permissionId = window.setInterval(refreshPermissions, 10000);
    const onFocus = () => {
      refreshPermissions();
      refreshHotkey();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(hotkeyId);
      window.clearInterval(permissionId);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  const reRequestAccessibility = async () => {
    await requestAccessibilityPermission();
    refreshPermissions();
  };

  const reRequestMicrophone = async () => {
    if (microphone === 'denied' || microphone === 'restricted') {
      await openSystemSettings('microphone');
      refreshPermissions();
      return;
    }
    const status = await requestMicrophonePermission();
    setMicrophone(status);
    if (status === 'denied' || status === 'restricted') {
      await openSystemSettings('microphone');
    }
    refreshPermissions();
  };

  const desc = capability?.requiresAccessibilityPermission
    ? t('settings.permissions.descAcc')
    : t('settings.permissions.descNoAcc');

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.permissions.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>
        {desc}
      </div>
      <SettingRow label={t('settings.permissions.micLabel')} desc={t('settings.permissions.micDesc')}>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <PermissionPill status={microphone} />
          {microphone !== 'granted' && microphone !== 'notApplicable' && microphone !== 'loading' && (
            <Btn variant="ghost" size="sm" onClick={reRequestMicrophone}>
              {microphone === 'denied' || microphone === 'restricted' ? t('settings.permissions.openSystem') : t('settings.permissions.grant')}
            </Btn>
          )}
        </div>
      </SettingRow>
      {capability?.requiresAccessibilityPermission && (
        <SettingRow label={t('settings.permissions.accLabel')} desc={t('settings.permissions.accDesc')}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <PermissionPill status={accessibility} />
            {accessibility !== 'granted' && accessibility !== 'notApplicable' && (
              <Btn variant="ghost" size="sm" onClick={reRequestAccessibility}>
                {t('settings.permissions.grant')}
              </Btn>
            )}
          </div>
        </SettingRow>
      )}
      <SettingRow
        label={t('settings.permissions.hotkeyLabel')}
        desc={capability ? t('settings.permissions.hotkeyDescWithAdapter', { adapter: adapterDisplayName(capability.adapter) }) : t('settings.permissions.hotkeyDescPlain')}
      >
        <div style={{ display: 'flex', gap: 8, alignItems: 'center', minWidth: 0 }}>
          <HotkeyStatusPill status={hotkey} />
          {hotkey?.message && (
            <span style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {hotkey.message}
            </span>
          )}
        </div>
      </SettingRow>
      <SettingRow label={t('settings.permissions.networkLabel')} desc={t('settings.permissions.networkDesc')}>
        <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.networkOk')}</Pill>
      </SettingRow>
    </Card>
  );
}

function PermissionPill({ status }: { status: PermissionStatus | 'loading' }) {
  const { t } = useTranslation();
  if (status === 'loading') {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status === 'granted') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.granted')}</Pill>;
  }
  if (status === 'notApplicable') {
    return <Pill tone="default">{t('settings.permissions.notApplicable')}</Pill>;
  }
  if (status === 'denied' || status === 'restricted') {
    return <Pill tone="outline">{t('settings.permissions.denied')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.indeterminate')}</Pill>;
}

function LanguageSection() {
  const { t } = useTranslation();
  const [pref, setPref] = useState<SupportedLocale | typeof FOLLOW_SYSTEM>(getLocalePreference());

  const apply = async (next: SupportedLocale | typeof FOLLOW_SYSTEM) => {
    setPref(next);
    await setLocalePreference(next);
  };

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t('settings.language.title')}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>{t('settings.language.desc')}</div>
      <SettingRow label={t('settings.language.label')} desc={t('settings.language.labelDesc')}>
        <select
          value={pref}
          onChange={e => apply(e.target.value as SupportedLocale | typeof FOLLOW_SYSTEM)}
          style={{ ...inputStyle, maxWidth: 220 }}
        >
          <option value={FOLLOW_SYSTEM}>{t('settings.language.followSystem')}</option>
          <option value="zh-CN">{t('settings.language.zh')}</option>
          <option value="en">{t('settings.language.en')}</option>
        </select>
      </SettingRow>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 8, lineHeight: 1.6 }}>
        {t('settings.language.restartHint')}
      </div>
    </Card>
  );
}

function AboutSection() {
  const { t } = useTranslation();
  const [qqCopied, setQqCopied] = useState(false);
  const qqCopiedRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (qqCopiedRef.current) clearTimeout(qqCopiedRef.current);
    };
  }, []);

  const copyQq = () => {
    navigator.clipboard?.writeText('1078960553');
    setQqCopied(true);
    if (qqCopiedRef.current) clearTimeout(qqCopiedRef.current);
    qqCopiedRef.current = window.setTimeout(() => setQqCopied(false), 1500);
  };

  return (
    <Card>
      <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 18 }}>
        <div
          style={{
            width: 52, height: 52, borderRadius: 12,
            background: 'linear-gradient(135deg, #0a0a0b 0%, #2563eb 100%)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: '#fff', fontSize: 22, fontWeight: 700, letterSpacing: '-0.02em',
          }}
        >OL</div>
        <div>
          <div style={{ fontSize: 16, fontWeight: 600 }}>OpenLess</div>
          <AboutUpdateControl tagline={t('settings.about.tagline')} />
        </div>
      </div>
      <SettingRow label={t('settings.about.source')}><Btn variant="ghost" size="sm" icon="link" onClick={() => openExternal('https://github.com/appergb/openless')}>GitHub</Btn></SettingRow>
      <SettingRow label={t('settings.about.docs')}><Btn variant="ghost" size="sm" icon="link" onClick={() => openExternal('https://github.com/appergb/openless#readme')}>README</Btn></SettingRow>
      <SettingRow label={t('settings.about.feedback')}><Btn variant="ghost" size="sm" icon="link" onClick={() => openExternal('https://github.com/appergb/openless/issues')}>GitHub Issues</Btn></SettingRow>
      <SettingRow label={t('settings.about.qq')} desc={t('settings.about.qqDesc')}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <kbd style={{
            padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
            borderRadius: 6, background: 'var(--ol-surface-2)',
            border: '0.5px solid var(--ol-line-strong)',
            boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
            color: 'var(--ol-ink-2)',
          }}>1078960553</kbd>
          <button onClick={copyQq} title={t('settings.about.copyQq')} style={iconBtnStyle}>
            <Icon name="copy" size={14} />
          </button>
          {qqCopied && <span style={{ fontSize: 11, color: 'var(--ol-ok)', whiteSpace: 'nowrap' }}>{t('common.copied')}</span>}
        </div>
      </SettingRow>
      <SettingRow label={t('settings.about.privacy')} desc={t('settings.about.privacyDesc')}>
        <Pill tone="default">{t('settings.about.localFirst')}</Pill>
      </SettingRow>
    </Card>
  );
}

export function AboutUpdateControl({ tagline }: { tagline: string }) {
  const { t } = useTranslation();
  const u = useAutoUpdate();
  return (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2 }}>
        <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>{tagline} · {APP_VERSION_LABEL}</span>
        <Btn variant="ghost" size="sm" onClick={u.checkForUpdates} disabled={u.checking || u.busy}>
          {u.checking ? t('settings.about.checkingUpdate') : t('settings.about.checkUpdateBtn')}
        </Btn>
      </div>
      {(u.status === 'none' || u.status === 'error') && (
        <div style={{ fontSize: 11, color: u.status === 'error' ? 'var(--ol-err)' : 'var(--ol-ink-4)', marginTop: 4 }}>
          {u.status === 'none' ? t('settings.about.upToDate') : t('settings.about.updateError')}
        </div>
      )}
      {isDialogStatus(u.status) && (
        <UpdateDialog
          status={u.status}
          version={u.version}
          progress={u.progress}
          downloaded={u.downloaded}
          contentLength={u.contentLength}
          onInstall={u.installUpdate}
          onClose={u.dismissDialog}
        />
      )}
    </>
  );
}

function HotkeyStatusPill({ status }: { status: HotkeyStatus | null }) {
  const { t } = useTranslation();
  if (!status) {
    return <Pill tone="default">{t('settings.permissions.checking')}</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />{t('settings.permissions.hotkeyInstalled')}</Pill>;
  }
  if (status.state === 'starting') {
    return <Pill tone="default">{t('settings.permissions.hotkeyStarting')}</Pill>;
  }
  return <Pill tone="outline">{t('settings.permissions.hotkeyFailed')}</Pill>;
}

function adapterDisplayName(adapter: HotkeyCapability['adapter'] | HotkeyStatus['adapter']) {
  if (adapter === 'macEventTap') return i18n.t('hotkey.adapter.macEventTap');
  if (adapter === 'windowsLowLevel') return i18n.t('hotkey.adapter.windowsLowLevel');
  return i18n.t('hotkey.adapter.rdev');
}
