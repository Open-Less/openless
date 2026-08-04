// 渠道卡片列表 —— LLM 润色与 ASR 语音转写共用同一套交互。
//
// 心智只有一条：**排序即优先级，列表里第一个启用的就是当前生效的渠道**。
// 开关关掉的渠道自动沉到列表末尾；后端不另存"当前选中"，避免"列表第一张是 A、
// 实际请求打的是 B"这种两处真相。详见 docs/provider-channels-plan.md。
//
// 卡片解决的两件事：同一家厂商可以存多把 key；key 之间切换只是拖一下顺序，
// 而不是把旧 key 覆盖掉。

import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Modal } from '../../components/ui/Modal';
import { SelectLite } from '../../components/ui/SelectLite';
import { detectOS } from '../../components/WindowChrome';
import {
  createChannel,
  deleteChannel,
  listChannels,
  readCredential,
  renameChannel,
  reorderChannels,
  setChannelEnabled,
  type Channel,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { Card } from '../_atoms';
import { ChannelCredentialFields, LLM_PRESETS, LOCAL_ASR_PROVIDER_IDS } from './ProvidersSection';
import { ASR_PRESETS, inputStyle, SectionTitle, Toggle } from './shared';

type ChannelKind = 'llm' | 'asr';

interface PresetOption {
  id: string;
  nameKey: string;
}

/** 「添加渠道」下拉里的供应商清单。本地引擎与 Codex OAuth 也在其中 —— 它们不是预置的
 *  固定卡片，而是和云端厂商一样由用户添加，只是编辑时没有 key / 地址字段。 */
function presetsFor(kind: ChannelKind, os: string): PresetOption[] {
  if (kind === 'llm') {
    return LLM_PRESETS.map(p => ({ id: p.id, nameKey: p.nameKey }));
  }
  return ASR_PRESETS.filter(p => {
    // Apple 语音是 macOS 专有。
    if (p.id === 'apple-speech') return os === 'mac';
    // 百炼的两个旧 id 是历史别名，统一入口是 `bailian`，不再让新卡片选到。
    if (p.id === 'bailian-qwen3-realtime' || p.id === 'bailian-fun-asr-flash') return false;
    return true;
  }).map(p => ({ id: p.id, nameKey: p.nameKey }));
}

function presetLabel(
  kind: ChannelKind,
  providerType: string,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const list: readonly { id: string; nameKey: string }[] =
    kind === 'llm' ? LLM_PRESETS : ASR_PRESETS;
  const preset = list.find(p => p.id === providerType);
  return preset
    ? t(`settings.providers.presets.${preset.nameKey}`)
    : providerType;
}

/** 卡片上模型那一行读的凭据账户 —— 与 ChannelCredentialFields 里保持一致。 */
function modelAccountFor(kind: ChannelKind): string {
  return kind === 'llm' ? 'ark.model_id' : 'asr.model';
}

function relativeTime(at: number, t: ReturnType<typeof useTranslation>['t']): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000) - at);
  if (seconds < 60) return t('settings.channels.justNow');
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return t('settings.channels.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t('settings.channels.hoursAgo', { count: hours });
  return t('settings.channels.daysAgo', { count: Math.floor(hours / 24) });
}

export function ChannelList({
  kind,
  autoCreateWhenEmpty = false,
}: {
  kind: ChannelKind;
  /** 新手引导用：列表为空时直接摊开添加表单，别让新用户对着空列表和一个加号发呆。 */
  autoCreateWhenEmpty?: boolean;
}) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const os = detectOS();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [models, setModels] = useState<Record<string, string>>({});
  const [loaded, setLoaded] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [dragId, setDragId] = useState<string | null>(null);
  // 只自动弹一次：用户取消掉之后不该再被弹窗追着跑。
  const autoOpenedRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const list = await listChannels(kind);
      setChannels(list);
      setLoaded(true);
      // 卡片上要显示每张卡当前的模型名 —— 凭据按渠道隔离，只能逐个读。
      // 渠道数量是个位数，并发读一轮的开销可以忽略。
      const account = modelAccountFor(kind);
      const entries = await Promise.all(
        list.map(async channel => {
          try {
            return [channel.id, (await readCredential(account, channel.id)) ?? ''] as const;
          } catch {
            return [channel.id, ''] as const;
          }
        }),
      );
      setModels(Object.fromEntries(entries));
    } catch (error) {
      console.error('[channels] failed to load', error);
      setLoaded(true);
    }
  }, [kind]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!autoCreateWhenEmpty || !loaded || autoOpenedRef.current) return;
    if (channels.length === 0) {
      autoOpenedRef.current = true;
      setCreating(true);
    }
  }, [autoCreateWhenEmpty, loaded, channels.length]);

  // 生效中的那张 = 第一个启用的（列表已按 order 排好）。
  const activeId = channels.find(c => c.enabled)?.id ?? null;

  const onToggle = async (channel: Channel) => {
    emitSaved('saving', t('common.saving'));
    try {
      await setChannelEnabled(kind, channel.id, !channel.enabled);
      await refresh();
      emitSaved('saved', t('common.saved'));
    } catch (error) {
      console.error('[channels] toggle failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const onDrop = async (targetId: string) => {
    if (!dragId || dragId === targetId) {
      setDragId(null);
      return;
    }
    const ids = channels.map(c => c.id);
    const from = ids.indexOf(dragId);
    const to = ids.indexOf(targetId);
    setDragId(null);
    if (from < 0 || to < 0) return;
    ids.splice(to, 0, ids.splice(from, 1)[0]);
    // 乐观更新：先按新顺序重排本地列表，避免拖完到刷新之间卡片跳回原位。
    setChannels(prev => ids.map(id => prev.find(c => c.id === id)!).filter(Boolean));
    try {
      await reorderChannels(kind, ids);
      await refresh();
      emitSaved('saved', t('common.saved'));
    } catch (error) {
      console.error('[channels] reorder failed', error);
      emitSaved('failed', t('common.operationFailed'));
      await refresh();
    }
  };

  const editing = channels.find(c => c.id === editingId) ?? null;

  return (
    <Card>
      <div style={{ marginBottom: 10 }}>
        <SectionTitle>
          {t(kind === 'llm' ? 'settings.providers.llmTitle' : 'settings.providers.asrTitle')}
        </SectionTitle>
      </div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}>
        {t('settings.channels.orderHint')}
      </div>

      {loaded && channels.length === 0 && (
        <div style={{ fontSize: 12.5, color: 'var(--ol-ink-4)', padding: '10px 0 14px', lineHeight: 1.6 }}>
          {t('settings.channels.empty')}
        </div>
      )}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        {channels.map(channel => {
          const isActive = channel.id === activeId;
          const label = channel.name.trim() || presetLabel(kind, channel.providerType, t);
          const model = models[channel.id] ?? '';
          const failed = channel.lastTest && !channel.lastTest.ok;
          return (
            <div
              key={channel.id}
              draggable
              onDragStart={() => setDragId(channel.id)}
              onDragOver={e => e.preventDefault()}
              onDrop={() => void onDrop(channel.id)}
              onDragEnd={() => setDragId(null)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                padding: '10px 12px',
                borderRadius: 10,
                border: `0.5px solid ${isActive ? 'var(--ol-blue)' : 'var(--ol-line-strong)'}`,
                background: channel.enabled ? 'var(--ol-surface)' : 'var(--ol-bg-2, transparent)',
                opacity: dragId === channel.id ? 0.5 : channel.enabled ? 1 : 0.62,
                cursor: 'grab',
              }}
            >
              <span style={{ color: 'var(--ol-ink-4)', fontSize: 13, flexShrink: 0 }} aria-hidden>⠿</span>
              <span
                aria-hidden
                style={{
                  width: 7,
                  height: 7,
                  borderRadius: '50%',
                  flexShrink: 0,
                  background: isActive ? 'var(--ol-ok)' : 'transparent',
                  border: isActive ? 'none' : '1px solid var(--ol-ink-4)',
                }}
              />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
                  <span style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</span>
                  {isActive && (
                    <span style={{ fontSize: 10.5, color: 'var(--ol-ok)' }}>
                      {t('settings.channels.inUse')}
                    </span>
                  )}
                </div>
                <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 2, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {/* 未命名时主标题已经是厂商名，副行再来一遍就成了重复的两行同名。 */}
                  {channel.name.trim() && <span>{presetLabel(kind, channel.providerType, t)}</span>}
                  {model && <span style={{ fontFamily: 'var(--ol-font-mono)' }}>{model}</span>}
                  {channel.lastTest?.ok && channel.lastTest.latencyMs != null && (
                    <span>{channel.lastTest.latencyMs}ms</span>
                  )}
                  {failed && (
                    <span style={{ color: 'var(--ol-warn)' }}>
                      {t('settings.channels.lastFailed', {
                        when: relativeTime(channel.lastTest!.at, t),
                      })}
                    </span>
                  )}
                </div>
              </div>
              <Toggle on={channel.enabled} onToggle={() => void onToggle(channel)} />
              <button
                onClick={() => setEditingId(channel.id)}
                title={t('settings.channels.edit')}
                aria-label={t('settings.channels.edit')}
                style={iconBtn}
              >
                <Icon name="chevRight" size={13} />
              </button>
            </div>
          );
        })}
      </div>

      <button onClick={() => setCreating(true)} style={{ ...addBtn, marginTop: channels.length ? 10 : 0 }}>
        ＋ {t('settings.channels.add')}
      </button>

      {creating && (
        <ChannelCreateModal
          kind={kind}
          presets={presetsFor(kind, os)}
          onClose={() => setCreating(false)}
          onCreated={async id => {
            setCreating(false);
            await refresh();
            setEditingId(id);
          }}
        />
      )}

      {editing && (
        <ChannelEditModal
          kind={kind}
          channel={editing}
          mobile={mobile}
          onClose={() => {
            setEditingId(null);
            void refresh();
          }}
          onChanged={refresh}
        />
      )}
    </Card>
  );
}

/**
 * 「服务 → AI 提供商」面板：LLM 与 ASR 两张渠道列表。
 *
 * 保留 `ProvidersSection` 这个名字与 `kind` 签名，让设置页 tabs 与新手引导的调用点
 * 不用改。渠道化之后它只是两个 <ChannelList> 的容器。
 */
export function ProvidersSection({
  kind = 'all',
  autoCreateWhenEmpty = false,
}: {
  kind?: 'all' | 'llm' | 'asr';
  autoCreateWhenEmpty?: boolean;
} = {}) {
  const { t } = useTranslation();
  return (
    <>
      {kind === 'all' && (
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}>
          {t('settings.providers.credentialStorageNotice')}
        </div>
      )}
      {(kind === 'all' || kind === 'llm') && (
        <ChannelList kind="llm" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
      {(kind === 'all' || kind === 'asr') && (
        <ChannelList kind="asr" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
    </>
  );
}

/** 新建：先定名字与供应商，创建拿到 id 之后才谈得上填凭据（凭据按渠道 id 作用域存）。 */
function ChannelCreateModal({
  kind,
  presets,
  onClose,
  onCreated,
}: {
  kind: ChannelKind;
  presets: PresetOption[];
  onClose: () => void;
  onCreated: (id: string) => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const [providerType, setProviderType] = useState(presets[0]?.id ?? '');
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!providerType || busy) return;
    setBusy(true);
    try {
      const id = await createChannel(kind, providerType, name.trim());
      await onCreated(id);
    } catch (error) {
      console.error('[channels] create failed', error);
      emitSaved('failed', t('common.operationFailed'));
      setBusy(false);
    }
  };

  return (
    <Modal onClose={onClose} width="min(460px, 100%)">
      <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ol-ink)', marginBottom: 14 }}>
        {t('settings.channels.createTitle')}
      </div>
      <label style={fieldLabel}>{t('settings.channels.providerLabel')}</label>
      <SelectLite
        value={providerType}
        onChange={setProviderType}
        options={presets.map(p => ({
          value: p.id,
          label: t(`settings.providers.presets.${p.nameKey}`),
        }))}
        ariaLabel={t('settings.channels.providerLabel')}
        style={{ ...inputStyle, width: '100%', marginBottom: 12 }}
      />
      <label style={fieldLabel}>{t('settings.channels.nameLabel')}</label>
      <input
        value={name}
        onChange={e => setName(e.target.value)}
        placeholder={t('settings.channels.namePlaceholder')}
        onKeyDown={e => {
          if (e.key === 'Enter') void submit();
        }}
        style={{ ...inputStyle, width: '100%', marginBottom: 18 }}
      />
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button onClick={onClose} style={ghostBtn}>{t('common.cancel')}</button>
        <button onClick={() => void submit()} disabled={busy || !providerType} style={primaryBtn}>
          {t('settings.channels.create')}
        </button>
      </div>
    </Modal>
  );
}

function ChannelEditModal({
  kind,
  channel,
  mobile,
  onClose,
  onChanged,
}: {
  kind: ChannelKind;
  channel: Channel;
  mobile: boolean;
  onClose: () => void;
  onChanged: () => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(channel.name);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const saveName = async () => {
    if (name === channel.name) return;
    try {
      await renameChannel(kind, channel.id, name.trim());
      await onChanged();
    } catch (error) {
      console.error('[channels] rename failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const remove = async () => {
    try {
      await deleteChannel(kind, channel.id);
      emitSaved('saved', t('common.saved'));
      onClose();
    } catch (error) {
      console.error('[channels] delete failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const isLocalEngine = LOCAL_ASR_PROVIDER_IDS.includes(channel.providerType);

  return (
    <Modal onClose={onClose} width={mobile ? 'min(560px, 100%)' : 'min(600px, 100%)'}>
      <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ol-ink)', marginBottom: 4 }}>
        {t('settings.channels.editTitle')}
      </div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 14 }}>
        {presetLabel(kind, channel.providerType, t)}
      </div>

      <label style={fieldLabel}>{t('settings.channels.nameLabel')}</label>
      <input
        value={name}
        onChange={e => setName(e.target.value)}
        onBlur={() => void saveName()}
        placeholder={t('settings.channels.namePlaceholder')}
        style={{ ...inputStyle, width: '100%', marginBottom: 14 }}
      />

      <ChannelCredentialFields
        kind={kind}
        providerType={channel.providerType}
        channelId={channel.id}
        onTested={() => void onChanged()}
      />

      {isLocalEngine && (
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginTop: 6 }}>
          {t('settings.channels.localEngineModelHint')}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', marginTop: 20, alignItems: 'center' }}>
        {confirmDelete ? (
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <span style={{ fontSize: 12, color: 'var(--ol-warn)' }}>
              {t('settings.channels.deleteConfirm')}
            </span>
            <button onClick={() => void remove()} style={dangerBtn}>{t('settings.channels.confirmDelete')}</button>
            <button onClick={() => setConfirmDelete(false)} style={ghostBtn}>{t('common.cancel')}</button>
          </div>
        ) : (
          <button onClick={() => setConfirmDelete(true)} style={ghostBtn}>
            {t('settings.channels.delete')}
          </button>
        )}
        <button onClick={onClose} style={primaryBtn}>{t('common.close')}</button>
      </div>
    </Modal>
  );
}

const fieldLabel: CSSProperties = {
  display: 'block',
  fontSize: 12,
  fontWeight: 500,
  color: 'var(--ol-ink-2)',
  marginBottom: 5,
};

const iconBtn: CSSProperties = {
  width: 30,
  height: 30,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-surface)',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  color: 'var(--ol-ink-3)',
  cursor: 'default',
  flexShrink: 0,
};

const addBtn: CSSProperties = {
  height: 34,
  padding: '0 14px',
  border: '0.5px dashed var(--ol-line-strong)',
  borderRadius: 9,
  background: 'transparent',
  color: 'var(--ol-ink-3)',
  cursor: 'default',
  fontSize: 12.5,
  fontWeight: 500,
  width: '100%',
};

const primaryBtn: CSSProperties = {
  height: 32,
  padding: '0 14px',
  border: '0.5px solid var(--ol-blue)',
  borderRadius: 8,
  background: 'var(--ol-blue)',
  color: '#fff',
  cursor: 'default',
  fontSize: 12.5,
  fontWeight: 500,
};

const ghostBtn: CSSProperties = {
  height: 32,
  padding: '0 14px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-surface)',
  color: 'var(--ol-ink-2)',
  cursor: 'default',
  fontSize: 12.5,
  fontWeight: 500,
};

const dangerBtn: CSSProperties = {
  ...ghostBtn,
  borderColor: 'var(--ol-warn)',
  color: 'var(--ol-warn)',
};
