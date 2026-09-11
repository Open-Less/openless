// LLM 与 ASR 共用的渠道列表和编辑器。
// Core 按排序选择第一个启用渠道；每个渠道独立保存凭据，支持同一供应商的多个账号。

import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { Icon } from '../../components/Icon';
import { Modal } from '../../components/ui/Modal';
import { SelectLite } from '../../components/ui/SelectLite';
import { detectOS, type OS } from '../../components/WindowChrome';
import { testLocalAsrChannel } from '../../lib/localAsr';
import {
  createChannel,
  deleteChannel,
  deleteChannelIfBlank,
  listChannels,
  listProviderDescriptors,
  readCredential,
  recordChannelTest,
  renameChannel,
  reorderChannels,
  setChannelEnabled,
  setChannelProviderType,
  setCredential,
  validateProviderCredentials,
  type Channel,
  type ProviderDescriptor,
} from '../../lib/ipc';
import { emitSaved } from '../../lib/savedEvent';
import { useExitMount } from '../../lib/useExitMount';
import {
  useMobileLayout,
  useReadableLayout,
  useConservativeLayout,
} from '../../lib/useMobileLayout';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';
import { getPlatformCapabilities } from '../../lib/platform';
import { Btn, Pill } from '../_atoms';
import {
  ChannelCredentialFields,
  ChannelFormRow,
  ChannelSectionHeading,
  LLM_LABELS,
  OmniChannelSection,
} from './ProvidersSection';
import { ProviderFormContext, useProviderForm } from './ProviderForm';
import { ASR_LABELS, inputStyle } from './shared';
import { ChannelEditorHostContext } from './ChannelEditorHostContext';

type ChannelKind = 'llm' | 'asr';

interface PresetOption {
  id: string;
  nameKey: string;
  defaultEndpoint?: string;
  defaultModel?: string;
  authRequirement?: ProviderDescriptor['authRequirement'];
  staticModels?: string[];
  defaultRequestFormat?: ProviderDescriptor['defaultRequestFormat'];
  supportedRequestFormats?: ProviderDescriptor['supportedRequestFormats'];
}

/** 「添加渠道」下拉里的供应商清单。本地引擎与 Codex OAuth 也在其中 —— 它们不是预置的
 *  固定卡片，而是和云端厂商一样由用户添加，只是编辑时没有 key / 地址字段。 */
export function presetsFor(
  kind: ChannelKind,
  os: OS,
  supportsQwen3Mlx = true,
  currentProviderId?: string,
  descriptors: ProviderDescriptor[] = [],
): PresetOption[] {
  const descriptorPresets = descriptors.map((descriptor) => ({
    id: descriptor.providerType,
    nameKey: descriptor.labelKey,
    defaultEndpoint: descriptor.defaultEndpoint ?? undefined,
    defaultModel: descriptor.defaultModel ?? undefined,
    authRequirement: descriptor.authRequirement,
    staticModels: descriptor.staticModels,
    defaultRequestFormat: descriptor.defaultRequestFormat,
    supportedRequestFormats: descriptor.supportedRequestFormats,
  }));
  if (kind === 'llm') return descriptorPresets;
  const available = descriptorPresets;
  const visible = available.filter((p) => {
    // 本地引擎严格按其实际支持的平台暴露；Linux / Android 不展示桌面专有实现。
    if (p.id === 'local-qwen3-mlx') return os === 'mac' && supportsQwen3Mlx;
    if (p.id === 'local-whisper' || p.id === 'apple-speech') return os === 'mac';
    if (p.id === 'local-qwen3-c') return os === 'mac' || os === 'linux';
    if (p.id === 'local-qwen3') return false;
    if (p.id === 'foundry-local-whisper' || p.id === 'sherpa-onnx-local') {
      return os === 'win';
    }
    // 百炼的两个旧 id 是历史别名，统一入口是 `bailian`，不再让新卡片选到。
    if (p.id === 'bailian-qwen3-realtime' || p.id === 'bailian-fun-asr-flash') return false;
    return true;
  });
  // 新建渠道继续隐藏历史别名；编辑已有渠道时把当前值补回，避免 Select value
  // 找不到对应 option 而显示为空。只接受注册表里已知的 preset，不放行任意字符串。
  if (currentProviderId && !visible.some((preset) => preset.id === currentProviderId)) {
    const current = available.find((preset) => preset.id === currentProviderId);
    if (current) visible.push(current);
  }
  return visible;
}

/** 只有从未发生用户交互的新建草稿才允许走空白回收。 */
export function shouldRecycleDraft(draftId: string | null, touched: boolean): boolean {
  return draftId != null && !touched;
}

/** OrcaRouter 渠道使用统一的无空格品牌名；只填空名称，不覆盖用户自定义命名。 */
export function defaultChannelNameForProvider(providerType: string, currentName: string): string {
  if (currentName.trim() || providerType !== 'orcarouter') return currentName;
  return 'OrcaRouter';
}

function presetLabel(
  kind: ChannelKind,
  providerType: string,
  t: ReturnType<typeof useTranslation>['t'],
  descriptors: ProviderDescriptor[],
): string {
  const descriptor = descriptors.find((item) => item.providerType === providerType);
  if (descriptor) return t(`settings.providers.presets.${descriptor.labelKey}`);
  const list: readonly { id: string; nameKey: string }[] = kind === 'llm' ? LLM_LABELS : ASR_LABELS;
  const preset = list.find((p) => p.id === providerType);
  return preset ? t(`settings.providers.presets.${preset.nameKey}`) : providerType;
}

/** 卡片上模型那一行读的凭据账户 —— 与 ChannelCredentialFields 里保持一致。 */
function modelAccountFor(kind: ChannelKind): string {
  return kind === 'llm' ? 'ark.model_id' : 'asr.model';
}

/**
 * 把后端的错误串压成按钮上放得下的短标签，且要**能指导行动**：
 * 401 是 key 不对、429 是被限流等会儿再说、超时是网络——用户看到才知道该改什么。
 */
function shortErrorLabel(raw: string | null, t: ReturnType<typeof useTranslation>['t']): string {
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
  const readable = useReadableLayout();
  const conservative = useConservativeLayout();
  const preferenceStack = readable || conservative;
  const os = detectOS();
  // 初值 false：getPlatformCapabilities() 的权威值是架构感知的（Apple Silicon /
  // Intel），以 os === 'mac' 起步会让 Intel Mac 打开下拉时闪现一次 MLX 预设，
  // 再由异步纠正消失。Apple Silicon 上 MLX 选项晚一帧出现，可接受。
  const [supportsQwen3Mlx, setSupportsQwen3Mlx] = useState(false);
  const [descriptors, setDescriptors] = useState<ProviderDescriptor[]>([]);
  const presets = presetsFor(kind, os, supportsQwen3Mlx, undefined, descriptors);
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

  useEffect(() => {
    void getPlatformCapabilities().then((caps) => setSupportsQwen3Mlx(caps.supportsLocalQwen3Mlx));
  }, []);

  useEffect(() => {
    void listProviderDescriptors(kind)
      .then(setDescriptors)
      .catch((error) => console.error('[channels] failed to load provider descriptors', error));
  }, [kind]);

  const refresh = useCallback(async () => {
    try {
      const list = await listChannels(kind);
      setChannels(list);
      setLoaded(true);
      // 广播给服务分类 tab：语言模型/语音识别是必配项，tab 上的红/黄状态点
      // 需要在任何增删改/启停后即时刷新。
      window.dispatchEvent(new CustomEvent('ol-channels-changed', { detail: { kind } }));
      // 卡片上要显示每张卡当前的模型名 —— 凭据按渠道隔离，只能逐个读。
      // 渠道数量是个位数，并发读一轮的开销可以忽略。
      const account = modelAccountFor(kind);
      const entries = await Promise.all(
        list.map(async (channel) => {
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
  const activeId = channels.find((c) => c.enabled)?.id ?? null;

  // ── 卡片上的验证 ──
  // 只在用户点的时候跑：验证是**真实的调用**（LLM 走一次真的润色请求、云端 ASR
  // 传一段静音音频、本地 ASR 加载并转写内置音频）。做成打开设置就全部自动验一遍的话，
  // 等于每次开设置都按卡片数烧一遍额度，还容易把自己撞进限流。
  const [testingIds, setTestingIds] = useState<Record<string, boolean>>({});

  const runTest = async (channel: Channel) => {
    if (testingIds[channel.id]) return;
    setTestingIds((prev) => ({ ...prev, [channel.id]: true }));
    const started = performance.now();
    try {
      const isGenericLocalAsr =
        kind === 'asr' &&
        [
          'local-qwen3',
          'local-qwen3-mlx',
          'local-qwen3-c',
          'local-whisper',
          'apple-speech',
        ].includes(channel.providerType);
      const result = isGenericLocalAsr
        ? await testLocalAsrChannel(channel.id).then(() => ({ ok: true }))
        : await validateProviderCredentials(kind, channel.id);
      const latency = Math.round(performance.now() - started);
      await recordChannelTest(
        kind,
        channel.id,
        result.ok,
        result.ok ? latency : null,
        result.ok ? null : 'validateFailed',
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      try {
        await recordChannelTest(kind, channel.id, false, null, message);
      } catch (recordError) {
        console.error('[channels] failed to record test', recordError);
      }
    } finally {
      setTestingIds((prev) => ({ ...prev, [channel.id]: false }));
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

  // FLIP 动画比较更新前后的布局位置，平滑呈现增删与排序变化。
  // 正在拖动的行保留自己的 transform，避免滑位动画覆盖拖动态。
  const prevRowTops = useRef(new Map<string, number>());
  useLayoutEffect(() => {
    // 同样只量布局位置（offsetTop）：rect.top 会被上一帧仍在飞行的 FLIP
    // transform 污染，量出来的位移差是错的，动画本身也会跟着抖。
    const nextTops = new Map<string, number>();
    rowsRef.current.forEach((element, id) => nextTops.set(id, element.offsetTop));
    rowsRef.current.forEach((element, id) => {
      const current = nextTops.get(id);
      if (current == null) return;
      const previous = prevRowTops.current.get(id);
      const isDragging = dragIdRef.current === id;
      const lift = isDragging ? ' scale(1.012)' : '';
      if (previous == null) {
        element.animate(
          [
            { opacity: 0, transform: `translateY(-8px)${isDragging ? '' : ' scale(0.98)'}` },
            { opacity: 1, transform: 'none' },
          ],
          { duration: 260, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' },
        );
      } else if (Math.abs(previous - current) > 1) {
        element.animate(
          [
            { transform: `translateY(${previous - current}px)${lift}` },
            { transform: `translateY(0)${lift}` },
          ],
          { duration: 300, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' },
        );
      }
    });
    prevRowTops.current = nextTops;
  }, [channels]);

  const dragCleanupRef = useRef<(() => void) | null>(null);

  /** 按指针命中的行实时排序。使用 offsetTop 读取布局坐标，避免 FLIP 的
   * transform 改变命中结果，导致同一指针位置反复触发互换。 */
  const moveDragTo = (pointerY: number) => {
    const dragId = dragIdRef.current;
    if (!dragId) return;
    let targetId: string | null = null;
    for (const [id, element] of rowsRef.current) {
      if (id === dragId) continue;
      const parent = element.offsetParent as HTMLElement | null;
      let top: number;
      let bottom: number;
      let y: number;
      if (parent) {
        top = element.offsetTop;
        bottom = top + element.offsetHeight;
        y = pointerY - parent.getBoundingClientRect().top;
      } else {
        const rect = element.getBoundingClientRect();
        top = rect.top;
        bottom = rect.bottom;
        y = pointerY;
      }
      if (y >= top && y <= bottom) {
        targetId = id;
        break;
      }
    }
    if (!targetId || targetId === dragId) return;
    setChannels((prev) => {
      const from = prev.findIndex((c) => c.id === dragId);
      const to = prev.findIndex((c) => c.id === targetId);
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
    document.body.style.cursor = '';
    const dragId = dragIdRef.current;
    dragIdRef.current = null;
    setDraggingId(null);
    if (!dragId) return;
    swallowNextClick();
    const ids = channelsRef.current.map((c) => c.id);
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
    orderAtDragStartRef.current = channelsRef.current.map((c) => c.id);
    setDraggingId(id);
    // 拖动期间整页光标保持 grabbing：指针滑出手柄后也能看出「正在拖」。
    document.body.style.cursor = 'grabbing';

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

  // 组件卸载（比如关掉设置面板）时别把 window 监听 / grabbing 光标留在外面。
  useEffect(
    () => () => {
      dragCleanupRef.current?.();
      document.body.style.cursor = '';
    },
    [],
  );

  const editingChannel = channels.find((c) => c.id === (draftId ?? editingId)) ?? null;
  const isDraft = draftId != null;

  // 弹窗退场门控：closing 动画期间保留最后一次
  // 打开的 channel/isDraft，避免动画播一半内容先消失、标题从「添加」闪回「编辑」。
  const dialogMount = useExitMount(editingChannel !== null);
  const lastDialogRef = useRef<{ channel: Channel; isDraft: boolean } | null>(null);
  if (editingChannel) lastDialogRef.current = { channel: editingChannel, isDraft };
  const dialogChannel = editingChannel ?? lastDialogRef.current?.channel ?? null;
  const dialogIsDraft = editingChannel ? isDraft : (lastDialogRef.current?.isDraft ?? false);

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
    <section
      aria-label={t(`settings.channels.${kind}Title`)}
      style={{ minWidth: 0, marginBottom: 24 }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'flex-start',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 16,
          marginBottom: 20,
        }}
      >
        <div style={{ flex: '1 1 260px', minWidth: 0 }}>
          <h2 style={{ margin: 0, color: 'var(--ol-ink)', fontSize: 17, fontWeight: 600 }}>
            {t(`settings.channels.${kind}Title`)}
          </h2>
          <p
            style={{ margin: '7px 0 0', fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.65 }}
          >
            {t('settings.channels.orderHint')}
          </p>
        </div>
        <Btn
          variant="blue"
          icon="plus"
          disabled={creatingBusy || presets.length === 0}
          onClick={() => void startCreate()}
        >
          {creatingBusy ? t('common.loading') : t('settings.channels.add')}
        </Btn>
      </div>

      {!loaded && (
        <div role="status" style={emptyStyle}>
          {t('common.loading')}
        </div>
      )}
      {loaded && channels.length === 0 && (
        <div style={emptyStyle}>
          <Icon
            name={kind === 'llm' ? 'sparkle' : 'mic'}
            size={24}
            style={{ color: 'var(--ol-blue)', marginBottom: 10 }}
          />
          <div>{t('settings.channels.empty')}</div>
        </div>
      )}

      {/* 生效渠道不再用蓝底 + 左侧竖条（「当前使用」徽章已经说明问题，
          整行染色太花哨）；行改为圆角卡片，选中态只用中性灰底 + 细描边。 */}
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
          paddingTop: channels.length ? 12 : 0,
        }}
      >
        {channels.map((channel) => {
          const isActive = channel.id === activeId;
          const providerLabel = presetLabel(kind, channel.providerType, t, descriptors);
          const label = channel.name.trim() || providerLabel;
          const model = models[channel.id] ?? '';
          const localEngine =
            descriptors.find((item) => item.providerType === channel.providerType)
              ?.authRequirement === 'none';
          return (
            <div
              key={channel.id}
              ref={(element) => {
                if (element) rowsRef.current.set(channel.id, element);
                else rowsRef.current.delete(channel.id);
              }}
              style={{
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                gap: '14px 20px',
                padding: '14px 12px',
                borderRadius: 12,
                // 拖动态：轻微抬升（scale + 大阴影 + 强描边 + 提高层级），
                // 让「哪张在被拖、拖到哪了」一目了然。
                border: '0.5px solid',
                borderColor:
                  draggingId === channel.id
                    ? 'var(--ol-line-strong)'
                    : isActive
                      ? 'var(--ol-line)'
                      : 'transparent',
                background: isActive ? 'var(--ol-surface-2)' : 'transparent',
                position: 'relative',
                zIndex: draggingId === channel.id ? 2 : undefined,
                transform: draggingId === channel.id ? 'scale(1.012)' : undefined,
                boxShadow: draggingId === channel.id ? 'var(--ol-shadow-lg)' : undefined,
                opacity: draggingId === channel.id ? 0.96 : 1,
                transition: draggingId
                  ? undefined
                  : 'background 0.16s var(--ol-motion-quick), border-color 0.16s var(--ol-motion-quick), transform 0.18s var(--ol-motion-spring), box-shadow 0.18s var(--ol-motion-soft)',
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: 10,
                  minWidth: 0,
                  flex: preferenceStack ? '1 1 100%' : '1 1 260px',
                }}
              >
                <span
                  onPointerDown={(e) => onDragHandleDown(e, channel.id)}
                  onClick={(e) => e.stopPropagation()}
                  title={t('settings.channels.dragHint')}
                  aria-label={t('settings.channels.dragHint')}
                  style={{
                    color: draggingId === channel.id ? 'var(--ol-ink)' : 'var(--ol-ink-4)',
                    fontSize: 18,
                    flexShrink: 0,
                    cursor: draggingId === channel.id ? 'grabbing' : 'grab',
                    touchAction: 'none',
                    padding: '0 4px',
                    userSelect: 'none',
                    transition: 'color 0.16s var(--ol-motion-quick)',
                  }}
                >
                  ⠿
                </span>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 8 }}>
                    <span
                      style={{
                        fontSize: 14,
                        fontWeight: 600,
                        color: 'var(--ol-ink)',
                        overflowWrap: 'anywhere',
                      }}
                    >
                      {label}
                    </span>
                    {isActive && (
                      <Pill tone="blue" size="sm">
                        {t('settings.channels.current')}
                      </Pill>
                    )}
                    {!channel.enabled && (
                      <Pill tone="outline" size="sm">
                        {t('settings.channels.disabled')}
                      </Pill>
                    )}
                  </div>
                  <div
                    style={{
                      fontSize: 12,
                      color: 'var(--ol-ink-3)',
                      marginTop: 6,
                      lineHeight: 1.6,
                      overflowWrap: 'anywhere',
                    }}
                  >
                    {channel.name.trim() && <span>{providerLabel} · </span>}
                    <span style={{ fontFamily: model ? 'var(--ol-font-mono)' : undefined }}>
                      {model ||
                        t(
                          localEngine
                            ? 'settings.channels.localModelManaged'
                            : 'settings.channels.modelNotSet',
                        )}
                    </span>
                  </div>
                  <ChannelTestResult
                    channel={channel}
                    testing={Boolean(testingIds[channel.id])}
                    t={t}
                  />
                </div>
              </div>
              <div
                className={conservative ? 'ol-conservative-stack' : undefined}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  flexWrap: 'wrap',
                  gap: 8,
                  marginLeft: preferenceStack ? 0 : 30,
                  width: preferenceStack ? '100%' : undefined,
                }}
              >
                <Btn
                  size="sm"
                  disabled={Boolean(testingIds[channel.id])}
                  onClick={() => void runTest(channel)}
                >
                  {t(
                    testingIds[channel.id]
                      ? 'settings.channels.verifying'
                      : 'settings.channels.verify',
                  )}
                </Btn>
                <button
                  type="button"
                  role="switch"
                  aria-checked={channel.enabled}
                  aria-label={t('settings.channels.enabledFor', { name: label })}
                  onClick={() => void onToggle(channel)}
                  style={{ ...ghostBtn, display: 'inline-flex', alignItems: 'center', gap: 7 }}
                >
                  <span
                    aria-hidden="true"
                    style={{
                      width: 24,
                      height: 14,
                      borderRadius: 999,
                      background: channel.enabled ? 'var(--ol-blue)' : 'var(--ol-toggle-off-bg)',
                      position: 'relative',
                    }}
                  >
                    <span
                      style={{
                        position: 'absolute',
                        top: 2,
                        left: channel.enabled ? 12 : 2,
                        width: 10,
                        height: 10,
                        borderRadius: 999,
                        background: 'var(--ol-toggle-knob)',
                      }}
                    />
                  </span>
                  {t('settings.channels.enabled')}
                </button>
                <button
                  type="button"
                  className="ol-channel-edit-button"
                  onClick={() => setEditingId(channel.id)}
                >
                  <Icon name="pencil" size={14} />
                  {t('settings.channels.edit')}
                  <Icon name="chevRight" size={14} />
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {dialogMount.mounted && dialogChannel && (
        <ChannelModal
          key={dialogChannel.id}
          kind={kind}
          channel={dialogChannel}
          presets={presetsFor(kind, os, supportsQwen3Mlx, dialogChannel.providerType, descriptors)}
          isDraft={dialogIsDraft}
          mobile={mobile}
          closing={dialogMount.closing}
          onClose={() => void closeModal()}
          onChanged={refresh}
          onUserMutation={markDraftTouched}
        />
      )}
    </section>
  );
}

/** Selection and the last manual test are separate facts. The action keeps a
 * stable label; result, elapsed time and age remain readable alongside it. */
function ChannelTestResult({
  channel,
  testing,
  t,
}: {
  channel: Channel;
  testing: boolean;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const last = channel.lastTest;
  const stale = last != null && Math.floor(Date.now() / 1000) - last.at > STALE_TEST_SECONDS;
  const passed = last?.ok;
  const elapsed = last?.latencyMs;
  return (
    <div
      role="status"
      style={{
        display: 'flex',
        alignItems: 'baseline',
        flexWrap: 'wrap',
        gap: '3px 8px',
        marginTop: 7,
        fontSize: 11.5,
        lineHeight: 1.6,
        color: 'var(--ol-ink-3)',
      }}
    >
      <span>{t('settings.channels.lastCheck')}</span>
      {testing ? (
        <span>{t('settings.channels.verifying')}</span>
      ) : !last ? (
        <span>{t('settings.channels.notVerified')}</span>
      ) : (
        <>
          <span
            style={{
              color: stale ? 'var(--ol-ink-3)' : passed ? 'var(--ol-ok)' : 'var(--ol-warn)',
            }}
          >
            {passed
              ? t('settings.channels.passed')
              : t('settings.channels.failed', { reason: shortErrorLabel(last?.error ?? null, t) })}
          </span>
          {passed && elapsed != null && (
            <span>{t('settings.channels.elapsed', { ms: elapsed })}</span>
          )}
          {last && (
            <time
              dateTime={new Date(last.at * 1000).toISOString()}
              title={new Date(last.at * 1000).toLocaleString()}
            >
              {relativeTime(last.at, t)}
            </time>
          )}
          {stale && <span>{t('settings.channels.staleResult')}</span>}
        </>
      )}
    </div>
  );
}

/**
 * 为设置与新手引导提供 LLM/ASR 渠道列表；多模态管线启用时展示 Omni 配置。
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
        <div
          style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', lineHeight: 1.6, marginBottom: 10 }}
        >
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
 * 添加与编辑复用同一表单。设置中挂载为右侧子页，新手引导中使用独立弹窗。
 * 新建渠道先取得凭据所属的 ID，关闭时仅回收从未发生用户操作的空白草稿。
 */
function ChannelModal({
  kind,
  channel,
  presets,
  isDraft,
  mobile,
  closing = false,
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
  /** 退场中：透传给 Modal 反向播放入场动画（useExitMount 门控卸载）。 */
  closing?: boolean;
  onClose: () => void;
  onChanged: () => void | Promise<void>;
  /** 用户对草稿做了有意义的操作；必须在异步写入前同步触发。 */
  onUserMutation: () => void;
}) {
  const { t } = useTranslation();
  // 关闭 / 换供应商前收敛渠道表单未落盘的异步写入（#1044 语义）。
  const form = useProviderForm();
  const [name, setName] = useState(channel.name);
  const [providerType, setProviderType] = useState(channel.providerType);
  const [changingProvider, setChangingProvider] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  const nameId = useId();
  const editorHost = useContext(ChannelEditorHostContext);
  const embedded = Boolean(editorHost?.container && editorHost.background);
  const closeRequestedRef = useRef(false);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const requestClose = () => {
    if (closeRequestedRef.current) return;
    void form.finish(() => {
      if (closeRequestedRef.current) return;
      closeRequestedRef.current = true;
      onCloseRef.current();
    });
  };

  useEffect(() => {
    const opener = document.activeElement;
    const dialog = dialogRef.current;
    const parentDialog =
      opener instanceof HTMLElement ? opener.closest<HTMLElement>('[role="dialog"]') : null;
    const background = embedded
      ? editorHost?.background
      : parentDialog !== dialog
        ? parentDialog
        : null;
    const wasInert = background?.inert ?? false;
    (
      dialog?.querySelector<HTMLElement>('[role="combobox"], input:not([disabled])') ?? dialog
    )?.focus();
    // Keep body-portaled provider menus accessible while disabling the covered
    // settings surface. aria-modal would hide those existing sibling portals.
    if (background) background.inert = true;
    if (embedded) editorHost?.registerClose(requestClose);
    return () => {
      if (embedded) editorHost?.registerClose(null);
      if (background) background.inert = wasInert;
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, []);

  const onDialogKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const dialog = dialogRef.current;
    if (!dialog || event.defaultPrevented) return;
    if (event.target instanceof Element && event.target.closest('[role="dialog"]') !== dialog)
      return;
    if (event.key === 'Escape') {
      // SelectLite handles its open menu first. Never close both layers at once.
      if (dialog.querySelector('[role="combobox"][aria-expanded="true"]')) return;
      event.preventDefault();
      event.stopPropagation();
      requestClose();
    } else if (event.key === 'Tab') {
      const controls = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.tabIndex >= 0 && element.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first) {
        event.preventDefault();
        dialog.focus();
      } else if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === dialog)
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  };

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
      const preset = presets.find((item) => item.id === next);
      if (!preset) return;
      const endpointAccount = kind === 'llm' ? 'ark.endpoint' : 'asr.endpoint';
      const modelAccount = kind === 'llm' ? 'ark.model_id' : 'asr.model';
      if (next === 'orcarouter') {
        if (preset.defaultEndpoint)
          await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
        if (preset.defaultModel) await setCredential(modelAccount, preset.defaultModel, channel.id);
        return;
      }
      if (preset.defaultEndpoint && !(await readCredential(endpointAccount, channel.id))?.trim()) {
        await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
      }
      if (preset.defaultModel && !(await readCredential(modelAccount, channel.id))?.trim()) {
        await setCredential(modelAccount, preset.defaultModel, channel.id);
      }
    } catch (error) {
      console.error('[channels] failed to fill provider defaults', error);
    }
  };

  const changeProvider = async (next: string) => {
    const previous = providerType;
    onUserMutation();
    await form.finish(() => undefined);
    setChangingProvider(true);
    try {
      await setChannelProviderType(kind, channel.id, next);
      await fillProviderDefaults(next);
      setProviderType(next);
      const defaultName = defaultChannelNameForProvider(next, name);
      if (defaultName !== name) {
        await renameChannel(kind, channel.id, defaultName);
        setName(defaultName);
      }
      await onChanged();
    } catch (error) {
      console.error('[channels] change provider failed', error);
      setProviderType(previous);
      emitSaved('failed', t('common.operationFailed'));
    } finally {
      setChangingProvider(false);
    }
  };

  const remove = async () => {
    try {
      await deleteChannel(kind, channel.id);
      emitSaved('saved', t('common.saved'));
      requestClose();
    } catch (error) {
      console.error('[channels] delete failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  const descriptor = presets.find((item) => item.id === providerType);
  const isLocalEngine = descriptor?.authRequirement === 'none';

  const content = (
    <ProviderFormContext.Provider value={form}>
      <div
        ref={dialogRef}
        className={`ol-channel-dialog${embedded ? ' ol-channel-dialog-embedded' : ''}`}
        data-closing={closing ? 'true' : undefined}
        role="dialog"
        aria-label={t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <header className="ol-channel-dialog-header">
          {embedded ? (
            <button
              type="button"
              className="ol-settings-back"
              onClick={requestClose}
              aria-label={t('settings.channels.backToList')}
              title={t('settings.channels.backToList')}
            >
              <Icon name="chevLeft" size={19} />
            </button>
          ) : (
            <span className="ol-channel-dialog-icon">
              <Icon name={kind === 'llm' ? 'style' : 'mic'} size={22} />
            </span>
          )}
          <div className="ol-channel-dialog-heading">
            <div className="ol-channel-dialog-title">
              <h2>
                {t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
              </h2>
              <span>{t(`settings.channels.${kind}Title`)}</span>
            </div>
            <p>{t('settings.channels.autoSaveHint')}</p>
          </div>
          {!embedded && (
            <button
              type="button"
              className="ol-channel-dialog-close"
              onClick={requestClose}
              aria-label={t('common.close')}
            >
              <Icon name="close" size={18} />
            </button>
          )}
        </header>

        <div className="ol-channel-dialog-body ol-thinscroll">
          <div className="ol-channel-form-sheet">
            <ChannelSectionHeading icon="cloud" title={t('settings.channels.connectionTitle')} />
            <ChannelFormRow label={t('settings.channels.providerLabel')}>
              <SelectLite
                value={providerType}
                disabled={changingProvider || form.leaving}
                onChange={(next) => void changeProvider(next)}
                options={presets.map((p) => ({
                  value: p.id,
                  label: t(`settings.providers.presets.${p.nameKey}`),
                }))}
                ariaLabel={t('settings.channels.providerLabel')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
            </ChannelFormRow>
            <ChannelFormRow label={t('settings.channels.nameLabel')} htmlFor={nameId}>
              <input
                id={nameId}
                value={name}
                onChange={(e) => {
                  onUserMutation();
                  setName(e.target.value);
                }}
                onBlur={() => void saveName()}
                placeholder={t('settings.channels.namePlaceholder')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
              <p className="ol-channel-name-hint">{t('settings.channels.nameHint')}</p>
            </ChannelFormRow>

            {/* 模型列表、供应商特有字段与验证结果都留在同一个滚动区。 */}
            {!changingProvider && (
              <ChannelCredentialFields
                key={`${channel.id}:${providerType}`}
                kind={kind}
                providerType={providerType}
                channelId={channel.id}
                descriptor={descriptor}
                onTested={() => void onChanged()}
                onUserMutation={onUserMutation}
              />
            )}
            {isLocalEngine && (
              <p className="ol-channel-local-hint">{t('settings.channels.localEngineModelHint')}</p>
            )}
          </div>
        </div>

        <footer className="ol-channel-dialog-footer">
          {confirmDelete ? (
            <div
              className="ol-channel-delete-confirm"
              role="group"
              aria-label={t('settings.channels.delete')}
            >
              <p>{t('settings.channels.deleteConfirm')}</p>
              <button
                type="button"
                autoFocus
                className="ol-channel-delete-button"
                onClick={() => void remove()}
              >
                <Icon name="trash" size={15} />
                {t('settings.channels.confirmDelete')}
              </button>
              <button type="button" onClick={() => setConfirmDelete(false)} style={ghostBtn}>
                {t('common.cancel')}
              </button>
            </div>
          ) : (
            <button
              type="button"
              className="ol-channel-delete-button"
              onClick={() => setConfirmDelete(true)}
            >
              <Icon name="trash" size={15} />
              {t('settings.channels.delete')}
            </button>
          )}
          <div className="ol-channel-dialog-footer-end">
            <span className="ol-channel-autosave-hint">{t('modal.autoSaveHint')}</span>
            <Btn
              variant={embedded ? 'primary' : 'blue'}
              onClick={requestClose}
              disabled={form.leaving}
            >
              {t(embedded ? 'settings.channels.done' : 'common.close')}
            </Btn>
          </div>
        </footer>
      </div>
    </ProviderFormContext.Provider>
  );
  return embedded ? (
    createPortal(content, editorHost!.container!)
  ) : (
    <Modal
      onClose={requestClose}
      zIndex={1000}
      closing={closing}
      width={mobile ? '100%' : 'min(840px, 100%)'}
      style={{ padding: 0, overflow: 'hidden', maxHeight: 'calc(100dvh - 40px)' }}
    >
      {content}
    </Modal>
  );
}

const emptyStyle: CSSProperties = {
  padding: '28px 20px',
  textAlign: 'center',
  fontSize: 13,
  color: 'var(--ol-ink-3)',
  lineHeight: 1.7,
  borderTop: '1px solid var(--ol-line)',
  borderBottom: '1px solid var(--ol-line)',
};

const ghostBtn: CSSProperties = {
  height: 32,
  padding: '0 14px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8,
  background: 'var(--ol-control-solid)',
  color: 'var(--ol-ink-2)',
  cursor: 'pointer',
  fontSize: 12.5,
  fontWeight: 500,
};
