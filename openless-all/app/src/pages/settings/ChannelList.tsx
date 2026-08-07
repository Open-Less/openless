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
import { detectOS, type OS } from '../../components/WindowChrome';
import {
  createChannel,
  deleteChannel,
  deleteChannelIfBlank,
  listChannels,
  readCredential,
  recordChannelTest,
  renameChannel,
  reorderChannels,
  setChannelEnabled,
  setChannelProviderType,
  setCredential,
  validateProviderCredentials,
  type Channel,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useMobileLayout } from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { Card } from '../_atoms';
import {
  ChannelCredentialFields,
  LLM_PRESETS,
  LOCAL_ASR_PROVIDER_IDS,
  OmniChannelSection,
} from './ProvidersSection';
import { ASR_PRESETS, inputStyle, SectionTitle, Toggle } from './shared';

type ChannelKind = 'llm' | 'asr';

interface PresetOption {
  id: string;
  nameKey: string;
}

/** 「添加渠道」下拉里的供应商清单。本地引擎与 Codex OAuth 也在其中 —— 它们不是预置的
 *  固定卡片，而是和云端厂商一样由用户添加，只是编辑时没有 key / 地址字段。 */
export function presetsFor(kind: ChannelKind, os: OS): PresetOption[] {
  if (kind === 'llm') {
    return LLM_PRESETS.map(p => ({ id: p.id, nameKey: p.nameKey }));
  }
  return ASR_PRESETS.filter(p => {
    // 本地引擎严格按其实际支持的平台暴露；Linux / Android 不展示桌面专有实现。
    if (p.id === 'local-qwen3' || p.id === 'apple-speech') return os === 'mac';
    if (p.id === 'foundry-local-whisper' || p.id === 'sherpa-onnx-local') {
      return os === 'win';
    }
    // 百炼的两个旧 id 是历史别名，统一入口是 `bailian`，不再让新卡片选到。
    if (p.id === 'bailian-qwen3-realtime' || p.id === 'bailian-fun-asr-flash') return false;
    return true;
  }).map(p => ({ id: p.id, nameKey: p.nameKey }));
}

/** 只有从未发生用户交互的新建草稿才允许走空白回收。 */
export function shouldRecycleDraft(draftId: string | null, touched: boolean): boolean {
  return draftId != null && !touched;
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

/**
 * 把后端的错误串压成按钮上放得下的短标签，且要**能指导行动**：
 * 401 是 key 不对、429 是被限流等会儿再说、超时是网络——用户看到才知道该改什么。
 */
function shortErrorLabel(
  raw: string | null,
  t: ReturnType<typeof useTranslation>['t'],
): string {
  const message = (raw ?? '').trim();
  if (message.startsWith('providerHttpStatus:')) {
    return message.split(':')[1] || t('settings.channels.errGeneric');
  }
  // 裸状态码也认（历史记录里可能只存了 "401"）——状态码本身就是最好的短标签。
  if (/^[1-5]\d{2}$/.test(message)) return message;
  if (message === 'providerRequestTimeout' || message.includes('timeout')) {
    return t('settings.channels.errTimeout');
  }
  if (message === 'providerNetworkError') return t('settings.channels.errNetwork');
  if (message === 'endpointMustUseHttps' || message === 'endpointInvalid') {
    return t('settings.channels.errEndpoint');
  }
  if (message === 'llmModelMissing' || message === 'asrModelMissing') {
    return t('settings.channels.errModel');
  }
  return t('settings.channels.errGeneric');
}

/** 一天以前的验证结果只能算"旧消息"，褪色表示不保证现在还有效。 */
const STALE_TEST_SECONDS = 24 * 60 * 60;

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
  const presets = presetsFor(kind, os);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [models, setModels] = useState<Record<string, string>>({});
  const [loaded, setLoaded] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  /** 新建时先落一张草稿卡片（凭据必须按渠道 id 写入），弹窗直接编辑它。 */
  const [draftId, setDraftId] = useState<string | null>(null);
  /** 同步 ref 避免 blur 保存与关闭弹窗之间的 state 调度竞态。 */
  const draftTouchedRef = useRef(false);
  const [creatingBusy, setCreatingBusy] = useState(false);
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

  // ── 添加：一步到位 ──
  // 点「添加渠道」直接开编辑弹窗（供应商、名字、密钥、测试都在里面）。草稿卡片在
  // 后台先建出来只是因为凭据要按渠道 id 落盘；用户完全没有交互就关掉时才会被回收。
  // 一旦改过任何字段就保留，避免 blur/debounce 保存与关闭流程竞争删除卡片。
  const startCreate = useCallback(async () => {
    if (creatingBusy) return;
    setCreatingBusy(true);
    draftTouchedRef.current = false;
    try {
      const id = await createChannel(kind, presets[0]?.id ?? '', '');
      setDraftId(id);
      await refresh();
    } catch (error) {
      console.error('[channels] create failed', error);
      emitSaved('failed', t('common.operationFailed'));
    } finally {
      setCreatingBusy(false);
    }
  }, [creatingBusy, kind, presets, refresh, t]);

  useEffect(() => {
    if (!autoCreateWhenEmpty || !loaded || autoOpenedRef.current) return;
    if (channels.length === 0) {
      autoOpenedRef.current = true;
      void startCreate();
    }
  }, [autoCreateWhenEmpty, loaded, channels.length, startCreate]);

  // 生效中的那张 = 第一个启用的（列表已按 order 排好）。
  const activeId = channels.find(c => c.enabled)?.id ?? null;

  // ── 卡片上的验证 ──
  // 只在用户点的时候跑：验证是**真实的 API 调用**（LLM 走一次真的润色请求、ASR 会传
  // 一段静音音频上去）。做成打开设置就全部自动验一遍的话，等于每次开设置都按卡片数
  // 烧一遍额度，还容易把自己撞进限流。
  const [testingIds, setTestingIds] = useState<Record<string, boolean>>({});
  /** 刚验通过的短暂高亮（id → 延迟 ms），几秒后落回常驻的灰色数字。 */
  const [justPassed, setJustPassed] = useState<Record<string, number>>({});

  const runTest = async (channel: Channel) => {
    if (testingIds[channel.id]) return;
    setTestingIds(prev => ({ ...prev, [channel.id]: true }));
    const started = performance.now();
    try {
      const result = await validateProviderCredentials(kind, channel.id);
      const latency = Math.round(performance.now() - started);
      await recordChannelTest(
        kind,
        channel.id,
        result.ok,
        result.ok ? latency : null,
        result.ok ? null : 'validateFailed',
      );
      if (result.ok) {
        setJustPassed(prev => ({ ...prev, [channel.id]: latency }));
        window.setTimeout(() => {
          setJustPassed(prev => {
            const next = { ...prev };
            delete next[channel.id];
            return next;
          });
        }, 3000);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      try {
        await recordChannelTest(kind, channel.id, false, null, message);
      } catch (recordError) {
        console.error('[channels] failed to record test', recordError);
      }
    } finally {
      setTestingIds(prev => ({ ...prev, [channel.id]: false }));
      await refresh();
    }
  };

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

  // ── 拖拽排序 ──
  // 用 pointer 事件手写，**不用 HTML5 draggable**：Tauri 的 webview 默认开着
  // dragDropEnabled，会把 dragstart/drop 当成文件拖放吞掉，`draggable` 在打包后的
  // app 里根本不触发（浏览器里却是好的，最容易漏测）。pointer 方案还顺带让
  // Windows 与 Android 的行为保持一致。
  const rowsRef = useRef(new Map<string, HTMLDivElement>());
  const channelsRef = useRef<Channel[]>([]);
  const dragIdRef = useRef<string | null>(null);
  const orderAtDragStartRef = useRef<string[]>([]);
  const [draggingId, setDraggingId] = useState<string | null>(null);

  useEffect(() => {
    channelsRef.current = channels;
  }, [channels]);

  const dragCleanupRef = useRef<(() => void) | null>(null);

  /** 指针移到哪张卡片上，就把被拖的那张插到那个位置 —— 卡片实时跟手。 */
  const moveDragTo = (pointerY: number) => {
    const dragId = dragIdRef.current;
    if (!dragId) return;
    let targetId: string | null = null;
    for (const [id, element] of rowsRef.current) {
      const rect = element.getBoundingClientRect();
      if (pointerY >= rect.top && pointerY <= rect.bottom) {
        targetId = id;
        break;
      }
    }
    if (!targetId || targetId === dragId) return;
    setChannels(prev => {
      const from = prev.findIndex(c => c.id === dragId);
      const to = prev.findIndex(c => c.id === targetId);
      if (from < 0 || to < 0 || from === to) return prev;
      const next = [...prev];
      next.splice(to, 0, next.splice(from, 1)[0]);
      return next;
    });
  };

  /// 拖拽刚结束时浏览器还会补一个 click。设置弹窗的遮罩层上挂着 onClick={onClose}，
  /// 这个补发的 click 会把整个设置面板关掉（拖一次卡片、设置就没了）。在捕获阶段
  /// 吞掉紧随其后的那一个 click，200ms 内没等到就撤掉监听。
  const swallowNextClick = () => {
    const handler = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
    };
    window.addEventListener('click', handler, { capture: true, once: true });
    window.setTimeout(() => {
      window.removeEventListener('click', handler, { capture: true });
    }, 200);
  };

  const endDrag = async () => {
    dragCleanupRef.current?.();
    dragCleanupRef.current = null;
    const dragId = dragIdRef.current;
    dragIdRef.current = null;
    setDraggingId(null);
    if (!dragId) return;
    swallowNextClick();
    const ids = channelsRef.current.map(c => c.id);
    const before = orderAtDragStartRef.current;
    if (ids.length === before.length && ids.every((id, index) => id === before[index])) {
      return; // 顺序没变，不打扰后端
    }
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

  // 刻意**不用** setPointerCapture：它会把后续事件重定向到手柄，浏览器补发的 click
  // 于是落到设置弹窗的遮罩上，一拖就把设置关了。改用 window 级监听，事件目标不变。
  const onDragHandleDown = (event: React.PointerEvent<HTMLElement>, id: string) => {
    event.preventDefault();
    event.stopPropagation();
    dragIdRef.current = id;
    orderAtDragStartRef.current = channelsRef.current.map(c => c.id);
    setDraggingId(id);

    const onMove = (moveEvent: PointerEvent) => moveDragTo(moveEvent.clientY);
    const onUp = () => void endDrag();
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    window.addEventListener('pointercancel', onUp);
    dragCleanupRef.current = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      window.removeEventListener('pointercancel', onUp);
    };
  };

  // 组件卸载（比如关掉设置面板）时别把 window 监听留在外面。
  useEffect(() => () => dragCleanupRef.current?.(), []);

  const editingChannel =
    channels.find(c => c.id === (draftId ?? editingId)) ?? null;
  const isDraft = draftId != null;

  const markDraftTouched = () => {
    if (draftId != null) draftTouchedRef.current = true;
  };

  const closeModal = async () => {
    const id = draftId;
    const touched = draftTouchedRef.current;
    setDraftId(null);
    setEditingId(null);
    draftTouchedRef.current = false;
    if (shouldRecycleDraft(id, touched)) {
      // 只回收从未发生用户交互的草稿；一旦用户改过任何内容，异步保存无论成功与否
      // 都不得与关闭流程竞争删除这张卡片。
      try {
        await deleteChannelIfBlank(kind, id!);
      } catch (error) {
        console.error('[channels] blank cleanup failed', error);
      }
    }
    await refresh();
  };

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
              ref={element => {
                if (element) rowsRef.current.set(channel.id, element);
                else rowsRef.current.delete(channel.id);
              }}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                // 左侧补偿 2px 竖条与 0.5px 细边的宽度差，文字基线保持对齐。
                padding: isActive ? '10px 12px 10px 10.5px' : '10px 12px',
                borderRadius: 10,
                border: '0.5px solid var(--ol-line-strong)',
                // 「当前在用」只用一条竖条表达位置，**不用绿色也不用文字**：
                // 绿色和「生效中」会被读成"这张是健康的"，可它只代表排在最前面 ——
                // 一张 key 已经失效的卡片照样排第一。健康与否只有验证说了算。
                borderLeft: isActive
                  ? '2.5px solid var(--ol-blue)'
                  : '0.5px solid var(--ol-line-strong)',
                background: channel.enabled ? 'var(--ol-surface)' : 'var(--ol-bg-2, transparent)',
                opacity: draggingId === channel.id ? 0.55 : channel.enabled ? 1 : 0.62,
                boxShadow: draggingId === channel.id ? '0 6px 18px rgba(0,0,0,0.14)' : undefined,
                transition: draggingId ? undefined : 'opacity 0.16s var(--ol-motion-quick)',
              }}
            >
              <span
                onPointerDown={e => onDragHandleDown(e, channel.id)}
                onClick={e => e.stopPropagation()}
                title={t('settings.channels.dragHint')}
                aria-label={t('settings.channels.dragHint')}
                style={{
                  color: 'var(--ol-ink-4)',
                  fontSize: 13,
                  flexShrink: 0,
                  cursor: draggingId === channel.id ? 'grabbing' : 'grab',
                  // 触摸设备上按住手柄不要变成页面滚动。
                  touchAction: 'none',
                  padding: '2px 2px',
                  userSelect: 'none',
                }}
              >
                ⠿
              </span>
              <div style={{ minWidth: 0, flex: 1 }}>
                <span style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</span>
                {/* minHeight 保证「未命名 + 没验证过」的卡片不会比别的矮一截，
                    列表高度参差看起来像坏了。 */}
                <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 2, minHeight: 15, display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {/* 未命名时主标题已经是厂商名，副行再来一遍就成了重复的两行同名。 */}
                  {channel.name.trim() && <span>{presetLabel(kind, channel.providerType, t)}</span>}
                  {model && <span style={{ fontFamily: 'var(--ol-font-mono)' }}>{model}</span>}
                  {/* 验证结果什么时候来的 —— 让"这条结论会过期"这件事可见。 */}
                  {channel.lastTest && <span>{relativeTime(channel.lastTest.at, t)}</span>}
                </div>
              </div>
              <VerifyButton
                channel={channel}
                testing={Boolean(testingIds[channel.id])}
                justPassedMs={justPassed[channel.id]}
                onRun={() => void runTest(channel)}
                t={t}
              />
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

      <button
        onClick={() => void startCreate()}
        disabled={creatingBusy}
        style={{ ...addBtn, marginTop: channels.length ? 10 : 0 }}
      >
        ＋ {t('settings.channels.add')}
      </button>

      {editingChannel && (
        <ChannelModal
          kind={kind}
          channel={editingChannel}
          presets={presets}
          isDraft={isDraft}
          mobile={mobile}
          onClose={() => void closeModal()}
          onChanged={refresh}
          onUserMutation={markDraftTouched}
        />
      )}
    </Card>
  );
}

/**
 * 卡片上的验证按钮 —— **按钮自己就是结果容器**，不给结果另找地方摆文字。
 *
 * 通过时只显示延迟数字：它既说明"通了"，又带信息量（一眼看出哪张快）；
 * 再写一句"验证通过"是废话还占地方。失败则必须给出能指导行动的短标签
 * （401 改 key / 429 等会儿 / 超时查网络）。
 *
 * 宽度固定：让 `验证` → `284ms` → `✗ 401` 的文字变化不会把开关和箭头挤来挤去。
 */
function VerifyButton({
  channel,
  testing,
  justPassedMs,
  onRun,
  t,
}: {
  channel: Channel;
  testing: boolean;
  justPassedMs?: number;
  onRun: () => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const last = channel.lastTest;
  const stale =
    last != null && Math.floor(Date.now() / 1000) - last.at > STALE_TEST_SECONDS;

  let label: string;
  let color = 'var(--ol-ink-3)';
  if (testing) {
    label = '···';
    color = 'var(--ol-ink-4)';
  } else if (justPassedMs != null) {
    label = `✓ ${justPassedMs}ms`;
    color = 'var(--ol-ok)';
  } else if (!last) {
    label = t('settings.channels.verify');
  } else if (last.ok) {
    label = last.latencyMs != null ? `${last.latencyMs}ms` : '✓';
    // 旧结果褪成浅灰：不保证现在还有效。
    color = stale ? 'var(--ol-ink-4)' : 'var(--ol-ink-3)';
  } else {
    label = `✗ ${shortErrorLabel(last.error, t)}`;
    color = 'var(--ol-warn)';
  }

  return (
    <button
      onClick={onRun}
      disabled={testing}
      title={t('settings.channels.verifyHint')}
      style={{
        width: 76,
        height: 28,
        flexShrink: 0,
        border: '0.5px solid var(--ol-line-strong)',
        borderRadius: 7,
        background: 'var(--ol-surface)',
        color,
        cursor: 'default',
        fontSize: 11.5,
        fontWeight: 500,
        opacity: stale && !testing ? 0.75 : 1,
        overflow: 'hidden',
        whiteSpace: 'nowrap',
      }}
    >
      {label}
    </button>
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
  const { prefs } = useHotkeySettings();
  // 多模态管线接管（issue #902）：多模态模式下隐藏传统 llm/asr 渠道列表，
  // 凭据两套并存但停用，切回即恢复（与合并前 beta 语义一致）。
  const multimodalMode =
    prefs?.multimodalPipelineEnabled === true && prefs?.pipelineMode === 'multimodal';
  return (
    <>
      {kind === 'all' && <OmniChannelSection />}
      {kind === 'all' && !multimodalMode && (
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}>
          {t('settings.providers.credentialStorageNotice')}
        </div>
      )}
      {!multimodalMode && (kind === 'all' || kind === 'llm') && (
        <ChannelList kind="llm" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
      {!multimodalMode && (kind === 'all' || kind === 'asr') && (
        <ChannelList kind="asr" autoCreateWhenEmpty={autoCreateWhenEmpty} />
      )}
    </>
  );
}

/**
 * 添加与编辑共用的同一个弹窗 —— 供应商、名字、凭据、测试连通都在这一屏里。
 *
 * 刻意不做「先创建、再填凭据」的两步：那只是实现上需要先有渠道 id 才能写凭据，
 * 不该变成用户多点一次。
 */
function ChannelModal({
  kind,
  channel,
  presets,
  isDraft,
  mobile,
  onClose,
  onChanged,
  onUserMutation,
}: {
  kind: ChannelKind;
  channel: Channel;
  presets: PresetOption[];
  /** 新建流程中的草稿卡片：标题用「添加渠道」，未触碰时允许回收。 */
  isDraft: boolean;
  mobile: boolean;
  onClose: () => void;
  onChanged: () => void | Promise<void>;
  /** 用户对草稿做了有意义的操作；必须在异步写入前同步触发。 */
  onUserMutation: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(channel.name);
  const [providerType, setProviderType] = useState(channel.providerType);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const saveName = async () => {
    if (name.trim() === channel.name.trim()) return;
    try {
      await renameChannel(kind, channel.id, name.trim());
      await onChanged();
    } catch (error) {
      console.error('[channels] rename failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  // 换供应商后把 preset 默认 endpoint / model 写进**空槽**（不覆盖用户已填的自定义值）。
  // 渠道化后每张卡凭据独立，这里按卡片 id 读写；Codex OAuth / 本地引擎 / 自定义
  // OpenAI 兼容（baseUrl/model 为空）自然跳过。失败只记日志，不影响换厂商本身。
  const fillProviderDefaults = async (next: string) => {
    try {
      if (kind === 'llm') {
        const preset = LLM_PRESETS.find(p => p.id === next);
        if (!preset || preset.id === 'custom' || preset.id === 'codex_oauth') return;
        if (preset.baseUrl && !(await readCredential('ark.endpoint', channel.id))?.trim()) {
          await setCredential('ark.endpoint', preset.baseUrl, channel.id);
        }
        if (
          preset.modelPlaceholder &&
          !(await readCredential('ark.model_id', channel.id))?.trim()
        ) {
          await setCredential('ark.model_id', preset.modelPlaceholder, channel.id);
        }
        return;
      }
      const preset = ASR_PRESETS.find(p => p.id === next);
      if (!preset) return;
      if (preset.baseUrl && !(await readCredential('asr.endpoint', channel.id))?.trim()) {
        await setCredential('asr.endpoint', preset.baseUrl, channel.id);
      }
      if (preset.model && !(await readCredential('asr.model', channel.id))?.trim()) {
        await setCredential('asr.model', preset.model, channel.id);
      }
    } catch (error) {
      console.error('[channels] failed to fill provider defaults', error);
    }
  };

  const changeProvider = async (next: string) => {
    const previous = providerType;
    onUserMutation();
    setProviderType(next);
    try {
      await setChannelProviderType(kind, channel.id, next);
      await fillProviderDefaults(next);
      await onChanged();
    } catch (error) {
      console.error('[channels] change provider failed', error);
      setProviderType(previous);
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

  const isLocalEngine = LOCAL_ASR_PROVIDER_IDS.includes(providerType);

  return (
    <Modal onClose={onClose} width={mobile ? 'min(560px, 100%)' : 'min(600px, 100%)'}>
      <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--ol-ink)', marginBottom: 14 }}>
        {t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
      </div>

      <label style={fieldLabel}>{t('settings.channels.providerLabel')}</label>
      <SelectLite
        value={providerType}
        onChange={next => void changeProvider(next)}
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
        onChange={e => {
          onUserMutation();
          setName(e.target.value);
        }}
        onBlur={() => void saveName()}
        placeholder={t('settings.channels.namePlaceholder')}
        style={{ ...inputStyle, width: '100%', marginBottom: 14 }}
      />

      {/* key 决定：换供应商时整组凭据字段重挂载，读的是新厂商对应的槽位。 */}
      <ChannelCredentialFields
        key={`${channel.id}:${providerType}`}
        kind={kind}
        providerType={providerType}
        channelId={channel.id}
        onTested={() => void onChanged()}
        onUserMutation={onUserMutation}
      />

      {isLocalEngine && (
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginTop: 6 }}>
          {t('settings.channels.localEngineModelHint')}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, justifyContent: 'space-between', marginTop: 20, alignItems: 'center' }}>
        {confirmDelete ? (
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <span style={{ fontSize: 12, color: 'var(--ol-warn)' }}>
              {t('settings.channels.deleteConfirm')}
            </span>
            <button onClick={() => void remove()} style={dangerBtn}>
              {t('settings.channels.confirmDelete')}
            </button>
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
