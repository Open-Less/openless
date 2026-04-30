// Settings.tsx — ported verbatim from design_handoff_openless/pages.jsx::Settings.
// Internal sub-sections (Recording / Providers / Shortcuts / Permissions / About)
// keep their inline-style literals 1:1 with the source JSX.

import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { Icon } from '../components/Icon';
import { APP_VERSION_LABEL } from '../lib/appVersion';
import { getHotkeyStartStopLabel, getHotkeyTriggerLabel } from '../lib/hotkey';
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  getHotkeyStatus,
  openExternal,
  openSystemSettings,
  readCredential,
  requestAccessibilityPermission,
  requestMicrophonePermission,
  setActiveLlmProvider,
  setCredential,
} from '../lib/ipc';
import type {
  HotkeyCapability,
  HotkeyMode,
  HotkeyStatus,
  HotkeyTrigger,
  PermissionStatus,
} from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

interface SettingsProps {
  embedded?: boolean;
  initialSection?: SettingsSectionId;
}

export type SettingsSectionId = '录音' | '提供商' | '快捷键' | '权限' | '关于';

export function Settings({ embedded = false, initialSection = '录音' }: SettingsProps) {
  const [section, setSection] = useState<SettingsSectionId>(initialSection);
  const sections: SettingsSectionId[] = ['录音', '提供商', '快捷键', '权限', '关于'];

  useEffect(() => {
    setSection(initialSection);
  }, [initialSection]);

  return (
    <>
      {!embedded && (
        <PageHeader
          kicker="SETTINGS"
          title="设置"
          desc="录音方式、模型与语音提供商、快捷键、权限与关于信息——全部在这里。"
        />
      )}
      <div style={{ display: 'grid', gridTemplateColumns: embedded ? '120px 1fr' : '160px 1fr', gap: 18 }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {sections.map(s => (
            <button
              key={s}
              onClick={() => setSection(s)}
              style={{
                padding: '8px 12px', textAlign: 'left',
                fontSize: 13, color: section === s ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                background: section === s ? 'rgba(0,0,0,0.04)' : 'transparent',
                border: 0, borderRadius: 8, fontFamily: 'inherit', fontWeight: section === s ? 600 : 500,
                cursor: 'default',
              }}
            >
              {s}
            </button>
          ))}
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {section === '录音' && <RecordingSection />}
          {section === '提供商' && <ProvidersSection />}
          {section === '快捷键' && <ShortcutsSection />}
          {section === '权限' && <PermissionsSection />}
          {section === '关于' && <AboutSection />}
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
  const { prefs, capability, updatePrefs: savePrefs } = useHotkeySettings();

  if (!prefs || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>加载中…</div>
      </Card>
    );
  }

  const onTriggerChange = (trigger: HotkeyTrigger) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, trigger } });
  const onModeChange = (mode: HotkeyMode) =>
    savePrefs({ ...prefs, hotkey: { ...prefs.hotkey, mode } });
  const onShowCapsuleChange = (showCapsule: boolean) =>
    savePrefs({ ...prefs, showCapsule });

  const choices: Array<[HotkeyMode, string]> = [
    ['toggle', '切换式'],
    ['hold', '按住说话'],
  ];
  const hotkeyDesc = capability.requiresAccessibilityPermission
    ? '按下即开始捕获语音，全局生效。需要授予辅助功能权限。'
    : '按下即开始捕获语音，全局生效。无需额外辅助功能授权。';

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>录音</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>定义全局录音的快捷键与触发方式。</div>
      <SettingRow label="录音快捷键" desc={hotkeyDesc}>
        <select
          value={prefs.hotkey.trigger}
          onChange={e => onTriggerChange(e.target.value as HotkeyTrigger)}
          style={{
            ...inputStyle,
            maxWidth: 200,
            fontFamily: 'var(--ol-font-mono)',
          }}
        >
          {capability.availableTriggers.map(t => (
            <option key={t} value={t}>{getHotkeyTriggerLabel(t)}</option>
          ))}
        </select>
      </SettingRow>
      <SettingRow label="录音方式" desc="切换式 = 按一次开始、再按一次结束；按住说话 = 按住开始、松开结束。">
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
              }}
            >
              {l}
            </button>
          ))}
        </div>
      </SettingRow>
      <SettingRow label="录音胶囊" desc="录音 / 转写时在屏幕底部显示半透明胶囊。">
        <Toggle on={prefs.showCapsule} onToggle={onShowCapsuleChange} />
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

const LLM_PRESETS = [
  { id: 'ark',      name: 'ARK（火山方舟）', baseUrl: 'https://ark.cn-beijing.volces.com/api/v3', modelPlaceholder: 'deepseek-v3-2' },
  { id: 'deepseek', name: 'DeepSeek',        baseUrl: 'https://api.deepseek.com/v1',             modelPlaceholder: 'deepseek-chat' },
  { id: 'openai',   name: 'OpenAI',          baseUrl: 'https://api.openai.com/v1',               modelPlaceholder: 'gpt-4o' },
  { id: 'custom',   name: '自定义',           baseUrl: '',                                        modelPlaceholder: '' },
] as const;

type LlmPresetId = typeof LLM_PRESETS[number]['id'];

const ASR_DEFAULT_RESOURCE_ID = 'volc.bigasr.sauc.duration';

function ProvidersSection() {
  const { prefs, updatePrefs } = useHotkeySettings();
  const [llmProvider, setLlmProvider] = useState<LlmPresetId>('ark');

  useEffect(() => {
    if (!prefs) return;
    const known = LLM_PRESETS.find(x => x.id === prefs.activeLlmProvider);
    setLlmProvider(known ? known.id : 'custom');
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

  const preset = LLM_PRESETS.find(p => p.id === llmProvider) ?? LLM_PRESETS[LLM_PRESETS.length - 1];

  return (
    <>
      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>LLM 模型（润色）</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>
            OpenAI 兼容协议，支持多家供应商切换。
          </div>
        </div>
        <SettingRow label="供应商" desc="选择后将自动填入 Base URL 默认值。">
          <select
            value={llmProvider}
            onChange={e => onLlmProviderChange(e.target.value as LlmPresetId)}
            style={{ ...inputStyle, maxWidth: 200 }}
          >
            {LLM_PRESETS.map(p => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </SettingRow>
        <CredentialField key={`${llmProvider}:api_key`} label="API Key" account="ark.api_key" mono mask />
        <CredentialField key={`${llmProvider}:endpoint`} label="Base URL" account="ark.endpoint"
          placeholder={preset.baseUrl || 'https://your-endpoint/v1'} />
        <CredentialField key={`${llmProvider}:model`} label="Model" account="ark.model_id"
          placeholder={preset.modelPlaceholder || 'model-name'} mono />
      </Card>

      <Card>
        <div style={{ marginBottom: 10 }}>
          <div style={{ fontSize: 13, fontWeight: 600 }}>ASR 语音（火山引擎）</div>
          <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>用于将口述实时转写为文本。</div>
        </div>
        <CredentialField label="App Key" account="volcengine.app_key" mono mask />
        <CredentialField label="Access Key" account="volcengine.access_key" mono mask />
        <CredentialField label="Resource ID" account="volcengine.resource_id" mono
          placeholder={ASR_DEFAULT_RESOURCE_ID} defaultValue={ASR_DEFAULT_RESOURCE_ID} />
      </Card>
    </>
  );
}

interface CredentialFieldProps {
  label: string;
  account: string;
  placeholder?: string;
  mono?: boolean;
  mask?: boolean;
  defaultValue?: string;
}

function CredentialField({ label, account, placeholder, mono, mask, defaultValue }: CredentialFieldProps) {
  const [value, setValue] = useState('');
  const [revealed, setRevealed] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    readCredential(account).then(v => setValue(v ?? ''));
  }, [account]);

  const save = async (v: string) => {
    await setCredential(account, v);
    setSaved(true);
    setTimeout(() => setSaved(false), 1200);
  };

  const onBlur = () => save(value);

  const fillDefault = async () => {
    if (!defaultValue) return;
    setValue(defaultValue);
    await save(defaultValue);
  };

  const inputType = mask && !revealed ? 'password' : 'text';

  return (
    <SettingRow label={label}>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', maxWidth: 420 }}>
        <input
          type={inputType}
          value={value}
          placeholder={placeholder}
          onChange={e => setValue(e.target.value)}
          onBlur={onBlur}
          style={{ ...inputStyle, fontFamily: mono ? 'var(--ol-font-mono)' : 'inherit' }}
        />
        {defaultValue && !value && (
          <button onClick={fillDefault} title="填入默认值" style={iconBtnStyle}>
            <Icon name="check" size={13} />
          </button>
        )}
        {mask && (
          <button
            onClick={() => setRevealed(r => !r)}
            title={revealed ? '隐藏' : '显示'}
            style={iconBtnStyle}
          >
            <Icon name="eye" size={14} />
          </button>
        )}
        <button
          onClick={() => navigator.clipboard?.writeText(value)}
          title="复制"
          style={iconBtnStyle}
          disabled={!value}
        >
          <Icon name="copy" size={14} />
        </button>
        {saved && (
          <span style={{ fontSize: 11, color: 'var(--ol-ok)', whiteSpace: 'nowrap' }}>已保存</span>
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
};
const iconBtnStyle: CSSProperties = {
  width: 32, height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  color: 'var(--ol-ink-3)', cursor: 'default', flexShrink: 0,
};

function ShortcutsSection() {
  const { hotkey, capability } = useHotkeySettings();

  if (!hotkey || !capability) {
    return (
      <Card>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-4)' }}>加载中…</div>
      </Card>
    );
  }

  const desc = capability.requiresAccessibilityPermission
    ? '所有快捷键全局生效，需要在权限设置中开启辅助功能。'
    : '所有快捷键全局生效。若无响应，请在权限页查看全局快捷键监听状态。';
  const rows: Array<[string, string]> = [
    ['开始 / 停止录音', getHotkeyStartStopLabel(hotkey)],
    ['取消本次录音', 'Esc'],
    ['胶囊确认插入', '点击右侧 ✓'],
    ['切换上一次风格', capability.requiresAccessibilityPermission ? '⌘ ⇧ S' : '暂未支持'],
    ['打开 OpenLess', capability.requiresAccessibilityPermission ? '⌘ ⇧ O' : '暂未支持'],
  ];
  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>快捷键速查</div>
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
  const [accessibility, setAccessibility] = useState<PermissionStatus | 'loading'>('loading');
  const [microphone, setMicrophone] = useState<PermissionStatus | 'loading'>('loading');
  const [hotkey, setHotkey] = useState<HotkeyStatus | null>(null);
  const { capability } = useHotkeySettings();

  const refresh = async () => {
    const [a, m] = await Promise.all([
      checkAccessibilityPermission(),
      checkMicrophonePermission(),
    ]);
    setAccessibility(a);
    setMicrophone(m);
    setHotkey(await getHotkeyStatus());
  };

  useEffect(() => {
    refresh();
    const id = window.setInterval(refresh, 1000);
    // 用户从系统设置切回来时立刻刷新（不等下一个 1s tick）
    const onFocus = () => refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(id);
      window.removeEventListener('focus', onFocus);
    };
  }, []);

  const reRequestAccessibility = async () => {
    await requestAccessibilityPermission();
    refresh();
  };

  const reRequestMicrophone = async () => {
    if (microphone === 'denied' || microphone === 'restricted') {
      await openSystemSettings('microphone');
      refresh();
      return;
    }
    const status = await requestMicrophonePermission();
    setMicrophone(status);
    if (status === 'denied' || status === 'restricted') {
      await openSystemSettings('microphone');
    }
    refresh();
  };

  const desc = capability?.requiresAccessibilityPermission
    ? 'OpenLess 需要以下系统权限才能正常工作。授权后通常需要完全退出 App 重启一次才生效。'
    : 'OpenLess 需要麦克风可用，并依赖全局快捷键监听状态判断 native hook 是否正常工作。';

  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>权限</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>
        {desc}
      </div>
      <SettingRow label="麦克风" desc="用于捕获你的语音输入。">
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <PermissionPill status={microphone} />
          {microphone !== 'granted' && microphone !== 'notApplicable' && microphone !== 'loading' && (
            <Btn variant="ghost" size="sm" onClick={reRequestMicrophone}>
              {microphone === 'denied' || microphone === 'restricted' ? '打开系统设置' : '授权'}
            </Btn>
          )}
        </div>
      </SettingRow>
      {capability?.requiresAccessibilityPermission && (
        <SettingRow label="辅助功能" desc="用于监听全局快捷键并将识别结果写入光标位置。">
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <PermissionPill status={accessibility} />
            {accessibility !== 'granted' && accessibility !== 'notApplicable' && (
              <Btn variant="ghost" size="sm" onClick={reRequestAccessibility}>
                授权
              </Btn>
            )}
          </div>
        </SettingRow>
      )}
      <SettingRow
        label="全局快捷键"
        desc={capability ? `当前适配器：${adapterDisplayName(capability.adapter)}。用于判断快捷键监听是否已经安装。` : '用于判断快捷键监听是否已经安装。'}
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
      <SettingRow label="网络" desc="云端 ASR / LLM 调用所必需。本地模式可关闭。">
        <Pill tone="ok"><Icon name="check" size={11} />可用</Pill>
      </SettingRow>
    </Card>
  );
}

function PermissionPill({ status }: { status: PermissionStatus | 'loading' }) {
  if (status === 'loading') {
    return <Pill tone="default">检查中…</Pill>;
  }
  if (status === 'granted') {
    return <Pill tone="ok"><Icon name="check" size={11} />已授权</Pill>;
  }
  if (status === 'notApplicable') {
    return <Pill tone="default">无需授权</Pill>;
  }
  if (status === 'denied' || status === 'restricted') {
    return <Pill tone="outline">未授权</Pill>;
  }
  return <Pill tone="outline">未确定</Pill>;
}

function AboutSection() {
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
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>自然说话，完美书写 · {APP_VERSION_LABEL}</div>
        </div>
      </div>
      <SettingRow label="检查更新"><Btn variant="ghost" size="sm" onClick={() => openExternal('https://github.com/appergb/openless/releases')}>打开 Releases</Btn></SettingRow>
      <SettingRow label="文档"><Btn variant="ghost" size="sm" icon="link" onClick={() => openExternal('https://github.com/appergb/openless#readme')}>README</Btn></SettingRow>
      <SettingRow label="反馈渠道"><Btn variant="ghost" size="sm" icon="link" onClick={() => openExternal('https://github.com/appergb/openless/issues')}>GitHub Issues</Btn></SettingRow>
      <SettingRow label="隐私" desc="所有识别结果仅保存在本机。云端 API 仅用于实时转写与润色，不会保留你的录音。">
        <Pill tone="default">本地优先</Pill>
      </SettingRow>
    </Card>
  );
}

function HotkeyStatusPill({ status }: { status: HotkeyStatus | null }) {
  if (!status) {
    return <Pill tone="default">检查中…</Pill>;
  }
  if (status.state === 'installed') {
    return <Pill tone="ok"><Icon name="check" size={11} />已安装</Pill>;
  }
  if (status.state === 'starting') {
    return <Pill tone="default">安装中…</Pill>;
  }
  return <Pill tone="outline">监听失败</Pill>;
}

function adapterDisplayName(adapter: HotkeyCapability['adapter'] | HotkeyStatus['adapter']) {
  if (adapter === 'macEventTap') return 'macOS Event Tap';
  if (adapter === 'windowsLowLevel') return 'Windows 低层键盘 hook';
  return 'rdev 监听器';
}
