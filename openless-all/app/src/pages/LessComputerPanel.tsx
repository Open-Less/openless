// LessComputerPanel.tsx — Less Computer 语音 Agent 浮窗（窗口 label = "less-computer"）。
//
// 结构 = 官方 shadcn base 组件文档 message-scroller-demo **同款骨架**：
//   MessageScrollerProvider → Card（CardHeader 标题/副行/CardAction ✕ →
//   CardContent(p-0) 内 MessageScroller / Empty 空状态 → CardFooter 内
//   InputGroup 输入组）。组件源码 1:1 来自官方 registry（components/chat/ui/，
//   仅按 CLI 规则改 import 路径），行为来自 @shadcn/react 官方 primitive。
//
// 「电脑操控」形态：不带头像 —— 用户指令 = Bubble align="end"（官方 bubble-demo
// 同款）；工具调用 = Marker + Spinner/✓ + shimmer（官方 marker-demo 同款）；
// 上下文压缩 = Marker separator；思考 = 与转译胶囊一模一样的 SiriGL 流体圆点。
//
// 事件流：`user`（fresh=true 清空重开）→ delta/tool/compaction/approval 交错 →
// completed（落成本）/ error / cancelled。浮窗首次创建时后端事件可能先于
// listener 注册到达（webview 冷加载），丢掉 user 事件后其余事件必须自愈补轮，
// 不能对空轮次静默丢弃（真机「后端在跑、前端一片空白」的根因）。
//
// 窗口固定尺寸（Rust 侧创建即定死 420×540），内容只在滚动框内滚动。
// 关闭：Esc / ✕ → less_computer_window_dismiss → 后端隐藏窗口。

import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUpIcon, CheckIcon, MessageCircleDashedIcon, XIcon } from 'lucide-react';
import {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
} from '../components/chat/ui/message-scroller';
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from '../components/chat/ui/card';
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '../components/chat/ui/empty';
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from '../components/chat/ui/input-group';
import { Marker, MarkerContent, MarkerIcon } from '../components/chat/ui/marker';
import { Bubble, BubbleContent } from '../components/chat/ui/bubble';
import { Button } from '../components/chat/ui/button';
import { Spinner } from '../components/chat/ui/spinner';
import { ThinkingOrb } from '../components/chat/avatars';
import { AssistantMarkdown } from '../components/chat/markdown';
import { useChatPanelLifecycle } from '../components/chat/lifecycle';
import { cn } from '../components/chat/lib/utils';
import {
  chatPanelFocusKeyboard,
  isTauri,
  lessComputerApprove,
  lessComputerSubmitText,
  lessComputerSync,
  lessComputerWindowDismiss,
} from '../lib/ipc';
import type { CapsulePayload, LessComputerEvent } from '../lib/types';
import '../components/chat/chat.css';

type RunStatus = 'idle' | 'working' | 'done' | 'error' | 'cancelled';

interface TextSegment {
  kind: 'text';
  content: string;
}

interface ToolSegment {
  kind: 'tool';
  name: string;
  /** 后端没有工具结束事件：下一个事件（delta/tool/approval/收尾）到达即视为结束。 */
  running: boolean;
}

interface ApprovalSegment {
  kind: 'approval';
  token: string;
  command: string;
  reason: string;
  /** 用户已点过的结果，决定按钮禁用态。undefined = 待处理。 */
  decision?: 'approved' | 'denied';
}

interface CompactionSegment {
  kind: 'compaction';
}

/** 助手输出流：文本 / 工具行 / 上下文压缩 / 审批卡按到达顺序排列（Codex 式交错）。 */
type Segment = TextSegment | ToolSegment | CompactionSegment | ApprovalSegment;

/** 一轮对话：用户一句 + 助手输出流 + 本轮收尾态。连续对话累积成数组。 */
interface Turn {
  user: string;
  segments: Segment[];
  status: RunStatus;
  errorMsg: string;
  costUsd: number | null;
}

function emptyTurn(user: string): Turn {
  return { user, segments: [], status: 'working', errorMsg: '', costUsd: null };
}

/**
 * 自愈：浮窗首次创建时 webview 冷加载，后端的 `user` 事件常常先于 listener
 * 注册被丢掉；随后的 delta/tool/收尾若发现没有任何轮次，就地补一轮（用户文案
 * 缺失，只是不显示指令气泡），保证输出照常渲染而不是永久空白。
 */
function ensureTurn(turns: Turn[]): Turn[] {
  return turns.length > 0 ? turns : [emptyTurn('')];
}

/** 对 turns 数组「最后一轮」做不可变更新（空数组先自愈补轮）。 */
function updateLastTurn(turns: Turn[], fn: (t: Turn) => Turn): Turn[] {
  const list = ensureTurn(turns);
  return [...list.slice(0, -1), fn(list[list.length - 1])];
}

/** 把流里还在扫光的工具行停下来（下一个事件到达 = 上一个工具已结束）。 */
function settleRunningTools(segments: Segment[]): Segment[] {
  if (!segments.some(s => s.kind === 'tool' && s.running)) return segments;
  return segments.map(s => (s.kind === 'tool' && s.running ? { ...s, running: false } : s));
}

// 浏览器预览（vite dev，非 Tauri）：?window=less-computer&demo=1 注入两轮演示对话
// （第一轮完成态含成本行；第二轮进行中，覆盖「文本 → 工具行 → 压缩行 → 进行中
// 工具行 → 审批卡」交错流与新轮次锚定），方便调样式。
function getPreviewTurns(): Turn[] {
  if (isTauri || typeof window === 'undefined') return [];
  if (new URLSearchParams(window.location.search).get('demo') !== '1') return [];
  return [
    {
      user: '看一下下载文件夹里最大的三个文件是什么',
      segments: [
        { kind: 'tool', name: 'Bash', running: false },
        {
          kind: 'text',
          content:
            '最大的三个文件：\n1. `Xcode_26.5.xip` — 12.4 GB\n2. `ubuntu-24.04.iso` — 5.8 GB\n3. `设计素材包.zip` — 2.1 GB',
        },
      ],
      status: 'done',
      errorMsg: '',
      costUsd: 0.012,
    },
    {
      user: '帮我把桌面上的截图整理到「本周素材」文件夹',
      segments: [
        { kind: 'text', content: '好的，我先看一下桌面上有哪些截图。' },
        { kind: 'tool', name: 'Bash', running: false },
        { kind: 'compaction' },
        { kind: 'text', content: '找到 6 张截图，正在移动并按日期重命名…' },
        { kind: 'tool', name: 'Bash', running: true },
        {
          kind: 'approval',
          token: 'demo',
          command: 'mv ~/Desktop/Screenshot*.png ~/Documents/本周素材/',
          reason: 'Moving files outside the working directory.',
        },
      ],
      status: 'working',
      errorMsg: '',
      costUsd: null,
    },
  ];
}

/** macOS movableByWindowBackground 拖动把手（header 区域整条可拖，普通箭头指针）。 */
const drag = { 'data-tauri-drag-region': true } as const;

/** 已应用事件的最大 seq。放模块级而不是 effect 闭包：StrictMode/HMR 重挂载时
 *  组件 state 保留，若水位归零会把同一批积压重放两遍、轮次翻倍。后端 seq 全局
 *  单调不回卷（新会话只清缓冲），webview 整页重载时本变量归零、恰好与「需要
 *  完整重放」对齐。 */
let lcAppliedSeq = 0;

export function LessComputerPanel() {
  const { t } = useTranslation();
  // 连续对话：每按一次说话键追加一轮（除非后端标记 fresh=新会话则清空重开）。
  const [turns, setTurns] = useState<Turn[]>(getPreviewTurns);
  // 新会话计数：fresh 时 +1，作为壳 key 重放入场动画 —— 浮窗是常驻 webview
  // （hide/show 复用），没有这个的话再次唤起时内容直接闪现，很突兀。
  const [sessionSeq, setSessionSeq] = useState(0);
  // 出现/消失动画：后端 show/hide 发 chat-panel:shown / chat-panel:closing。
  const { enterEpoch, closing } = useChatPanelLifecycle();

  // ── 后端事件订阅（mount 一次）────────────────────────────────────────
  //
  // 冷加载竞态补偿：webview 首次创建需要数百毫秒，后端在此期间 emit 的事件
  // （尤其首条 user —— 用户说的那句话）到不了 listener。协议：
  //   1) 先注册 listener，实时事件暂存 pending（不直接应用）；
  //   2) 调 less_computer_sync 拉后端缓冲，按 seq 升序全量重放；
  //   3) 放行 pending 与后续实时流，seq ≤ 已应用最大值的重复事件丢弃。
  // 无 seq 的事件（后端缓冲锁异常的降级路径）无条件应用。
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let synced = false;
    const pending: LessComputerEvent[] = [];
    const applyDeduped = (ev: LessComputerEvent) => {
      if (typeof ev.seq === 'number') {
        if (ev.seq <= lcAppliedSeq) return;
        lcAppliedSeq = ev.seq;
      }
      applyEvent(ev);
    };
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen<LessComputerEvent>('less-computer:event', event => {
          if (synced) applyDeduped(event.payload);
          else pending.push(event.payload);
        });
        if (cancelled) {
          handle();
          return;
        }
        unlisten = handle;
        const backlog = await lessComputerSync().catch(error => {
          console.error('[LessComputer] sync failed', error);
          return [] as LessComputerEvent[];
        });
        if (cancelled) return;
        for (const ev of backlog) applyDeduped(ev);
        synced = true;
        for (const ev of pending) applyDeduped(ev);
        pending.length = 0;
      } catch (error) {
        console.error('[LessComputer] listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const applyEvent = (ev: LessComputerEvent) => {
    switch (ev.kind) {
      case 'user': {
        // 一轮新对话。fresh=true（后端无可续会话→新会话）则清空历史重开；否则追加为后续轮次。
        setTurns(prev => (ev.fresh ? [emptyTurn(ev.text)] : [...prev, emptyTurn(ev.text)]));
        if (ev.fresh) setSessionSeq(seq => seq + 1);
        break;
      }
      case 'started':
        setTurns(prev => updateLastTurn(prev, tn => ({ ...tn, status: 'working' })));
        break;
      case 'delta':
        setTurns(prev =>
          updateLastTurn(prev, tn => {
            const segments = settleRunningTools(tn.segments);
            const last = segments[segments.length - 1];
            if (last?.kind === 'text') {
              return {
                ...tn,
                status: 'working',
                segments: [
                  ...segments.slice(0, -1),
                  { ...last, content: last.content + ev.text },
                ],
              };
            }
            return {
              ...tn,
              status: 'working',
              segments: [...segments, { kind: 'text', content: ev.text }],
            };
          }),
        );
        break;
      case 'tool':
        setTurns(prev =>
          updateLastTurn(prev, tn => ({
            ...tn,
            status: 'working',
            segments: [
              ...settleRunningTools(tn.segments),
              { kind: 'tool', name: ev.name, running: true },
            ],
          })),
        );
        break;
      case 'compaction':
        setTurns(prev =>
          updateLastTurn(prev, tn => ({
            ...tn,
            segments: [...settleRunningTools(tn.segments), { kind: 'compaction' }],
          })),
        );
        break;
      case 'approval':
        setTurns(prev =>
          updateLastTurn(prev, tn => ({
            ...tn,
            status: 'working',
            segments: [
              ...settleRunningTools(tn.segments),
              { kind: 'approval', token: ev.token, command: ev.command, reason: ev.reason },
            ],
          })),
        );
        break;
      case 'completed':
        setTurns(prev =>
          updateLastTurn(prev, tn => {
            let segments = settleRunningTools(tn.segments);
            // 正常情况最终文本已通过 delta 流出；只有整轮没有任何文本时才用
            // completed 的成品兜底（否则会把穿插的工具行冲掉）。
            if (ev.text && !segments.some(s => s.kind === 'text')) {
              segments = [...segments, { kind: 'text', content: ev.text }];
            }
            return { ...tn, segments, costUsd: ev.costUsd ?? null, status: 'done' };
          }),
        );
        break;
      case 'error':
        setTurns(prev =>
          updateLastTurn(prev, tn => ({
            ...tn,
            segments: settleRunningTools(tn.segments),
            errorMsg: ev.message,
            status: 'error',
          })),
        );
        break;
      case 'cancelled':
        setTurns(prev =>
          updateLastTurn(prev, tn => ({
            ...tn,
            segments: settleRunningTools(tn.segments),
            status: 'cancelled',
          })),
        );
        break;
    }
  };

  const onApproval = (token: string, approved: boolean) => {
    setTurns(prev =>
      prev.map(tn => ({
        ...tn,
        segments: tn.segments.map(s =>
          s.kind === 'approval' && s.token === token
            ? { ...s, decision: approved ? 'approved' : ('denied' as const) }
            : s,
        ),
      })),
    );
    void lessComputerApprove(token, approved);
  };

  const onClose = () => void lessComputerWindowDismiss();

  // ── Esc 关闭 ────────────────────────────────────────────────────────
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void lessComputerWindowDismiss();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  const working = turns.some(tn => tn.status === 'working');

  // ── 官方 message-scroller-demo 同款骨架 ─────────────────────────────
  return (
    <MessageScrollerProvider
      autoScroll
      defaultScrollPosition="last-anchor"
      scrollPreviousItemPeek={18}
    >
      <Card
        key={`${sessionSeq}-${enterEpoch}`}
        className={cn(
          'olchat-shell olchat-shell-in h-screen w-full gap-0',
          closing && 'olchat-shell-out',
        )}
      >
        <CardHeader {...drag} className="gap-1 border-b">
          <CardTitle {...drag}>{t('lessComputer.title')}</CardTitle>
          <CardDescription {...drag}>
            {working ? (
              <span className="shimmer" role="status">
                {t('lessComputer.working')}
              </span>
            ) : (
              t('lessComputer.subtitle')
            )}
          </CardDescription>
          <CardAction>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onClose}
              onMouseDown={event => {
                event.preventDefault();
                event.stopPropagation();
              }}
              title={t('lessComputer.closeTooltip')}
              aria-label={t('lessComputer.closeTooltip')}
            >
              <XIcon />
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex-1 overflow-hidden p-0">
          {turns.length === 0 ? (
            <Empty className="h-full">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <MessageCircleDashedIcon />
                </EmptyMedia>
                <EmptyTitle>{t('lessComputer.title')}</EmptyTitle>
                <EmptyDescription>{t('lessComputer.subtitle')}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <MessageScroller>
              <MessageScrollerViewport>
                <MessageScrollerContent
                  aria-busy={working || undefined}
                  className="p-(--card-spacing)"
                >
                  {turns.map((turn, ti) => (
                    <TurnView key={ti} index={ti} turn={turn} onApproval={onApproval} t={t} />
                  ))}
                </MessageScrollerContent>
              </MessageScrollerViewport>
              <MessageScrollerButton aria-label={t('lessComputer.jumpToLatest')} />
            </MessageScroller>
          )}
        </CardContent>
        <CardFooter className="flex-col gap-2">
          <Composer working={working} t={t} />
        </CardFooter>
      </Card>
    </MessageScrollerProvider>
  );
}

// ── 底部输入区：官方 demo 同款 InputGroup，打字 + 语音两种形式完整 ────
//
// · 打字：单行输入 + 右下发送（官方 demo 的 block-end addon 布局）；Enter 走
//   表单提交；IME 组合中的 Enter（选字确认）不触发（isComposing/keyCode 229
//   守卫）。点进输入框时 chat_panel_focus_keyboard 让非激活面板成为 key window
//   （不激活 app，主窗口不动）。
// · 语音：录音红光、转译思考黑光绕输入组一圈圈跑（olchat-ring），输入框本体
//   保持可见；指令落定（user 事件到、agent 开跑）即停 —— 监听 capsule:state
//   的 operating 会话获得状态，与屏幕下方胶囊同一数据源。
function Composer({
  working,
  t,
}: {
  working: boolean;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const [text, setText] = useState('');
  const [voice, setVoice] = useState<
    { state: 'recording'; level: number } | { state: 'thinking' } | null
  >(null);
  // 输入组环形光：录音红光 → 转译黑光 → 指令落定（agent 已在跑）即停。
  const ring =
    voice?.state === 'recording'
      ? 'recording'
      : voice?.state === 'thinking' && !working
        ? 'thinking'
        : undefined;
  // IME 组合期间的 Enter 是「选字确认」不是「发送」。keydown 里 isComposing
  // 已覆盖大部分场景，keyCode 229 兜底 WebKit 老行为。
  const composingRef = useRef(false);

  useEffect(() => {
    if (!isTauri) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen<CapsulePayload>('capsule:state', event => {
          const p = event.payload;
          if (p.operating !== true) return;
          if (p.state === 'recording') {
            setVoice({ state: 'recording', level: p.level ?? 0 });
          } else if (p.state === 'transcribing' || p.state === 'polishing') {
            setVoice(prev => (prev?.state === 'thinking' ? prev : { state: 'thinking' }));
          } else {
            setVoice(null);
          }
        });
        if (cancelled) handle();
        else unlisten = handle;
      } catch (error) {
        console.error('[LessComputer] capsule listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const send = () => {
    const trimmed = text.trim();
    if (!trimmed || working) return;
    setText('');
    void lessComputerSubmitText(trimmed);
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) {
      event.preventDefault();
    }
  };

  return (
    <form
      onSubmit={event => {
        event.preventDefault();
        send();
      }}
      className="w-full"
    >
      <InputGroup className="olchat-ring" data-ring={ring}>
        <InputGroupInput
          value={text}
          placeholder={t('lessComputer.inputPlaceholder')}
          onChange={event => setText(event.currentTarget.value)}
          onKeyDown={onKeyDown}
          onCompositionStart={() => {
            composingRef.current = true;
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
          }}
          onFocus={() => void chatPanelFocusKeyboard()}
          onPointerDown={() => void chatPanelFocusKeyboard()}
        />
        <InputGroupAddon align="block-end" className="pt-1">
          <InputGroupButton
            type="submit"
            variant="default"
            size="icon-sm"
            className="ml-auto"
            disabled={working || !text.trim()}
          >
            <ArrowUpIcon />
            <span className="sr-only">{t('lessComputer.send')}</span>
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
    </form>
  );
}

// ── 消息行 ────────────────────────────────────────────────────────────

/** 渲染单轮对话：用户气泡行（锚点，无头像）→ 输出流（文本 / Marker 工具行 /
 *  separator 压缩行 / 审批卡交错）→ 收尾（思考圆点 / 错误 / 花费）。
 *  自愈补出的轮次没有用户文案，跳过指令气泡、锚点落在输出行上。 */
function TurnView({
  index,
  turn,
  onApproval,
  t,
}: {
  index: number;
  turn: Turn;
  onApproval: (token: string, approved: boolean) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const hasUser = turn.user.trim().length > 0;
  const lastSegment = turn.segments[turn.segments.length - 1];
  // 思考指示只在流里没有「自带活动感」的内容时出现：整轮还没输出，或审批已决、
  // 后端正在带着授权重跑。进行中工具行的 Spinner + 扫光本身就是活动指示。
  const waiting =
    turn.status === 'working' &&
    (turn.segments.length === 0 ||
      (lastSegment?.kind === 'approval' && lastSegment.decision != null));
  const hasAssistantRow =
    turn.segments.length > 0 ||
    waiting ||
    turn.status === 'error' ||
    turn.status === 'cancelled' ||
    (turn.status === 'done' && turn.costUsd != null);
  return (
    <>
      {/* 用户指令 = 新轮次锚点行。电脑操控形态不带头像，右侧主色气泡。 */}
      {hasUser && (
        <MessageScrollerItem messageId={`t${index}-user`} scrollAnchor className="olchat-enter">
          <Bubble align="end">
            <BubbleContent>{turn.user}</BubbleContent>
          </Bubble>
        </MessageScrollerItem>
      )}
      {hasAssistantRow && (
        <MessageScrollerItem messageId={`t${index}-assistant`} scrollAnchor={!hasUser}>
          <div className="flex min-w-0 flex-col gap-3">
            {turn.segments.map((segment, i) => {
              if (segment.kind === 'text') {
                const streaming =
                  turn.status === 'working' && i === turn.segments.length - 1;
                return (
                  <div key={`s${i}`} className="olchat-enter">
                    <AssistantMarkdown markdown={segment.content} streaming={streaming} />
                  </div>
                );
              }
              if (segment.kind === 'tool') {
                return (
                  <ToolMarker
                    key={`s${i}`}
                    label={t('lessComputer.tool', { name: segment.name })}
                    running={segment.running}
                  />
                );
              }
              if (segment.kind === 'compaction') {
                return <CompactionMarker key={`s${i}`} label={t('lessComputer.compaction')} />;
              }
              return (
                <ApprovalCard key={segment.token} card={segment} onDecide={onApproval} t={t} />
              );
            })}
            {waiting && <WorkingRow label={t('lessComputer.working')} />}
            {turn.status === 'error' && (
              <Bubble variant="destructive" className="olchat-enter">
                <BubbleContent>{turn.errorMsg || t('lessComputer.error')}</BubbleContent>
              </Bubble>
            )}
            {turn.status === 'cancelled' && (
              <Marker>
                <MarkerContent className="font-mono text-[11px]">
                  {t('common.cancelled')}
                </MarkerContent>
              </Marker>
            )}
            {turn.status === 'done' && turn.costUsd != null && (
              <Marker>
                <MarkerContent className="font-mono text-[11px]">
                  {t('lessComputer.cost', { cost: turn.costUsd.toFixed(3) })}
                </MarkerContent>
              </Marker>
            )}
          </div>
        </MessageScrollerItem>
      )}
    </>
  );
}

/**
 * 工具调用标签：官方 marker-demo 同款 —— 进行中 = Marker role="status" +
 * MarkerIcon Spinner + MarkerContent shimmer；结束 = ✓ + 淡字。
 */
function ToolMarker({ label, running }: { label: string; running: boolean }) {
  return (
    <Marker className="olchat-enter" role={running ? 'status' : undefined}>
      <MarkerIcon>{running ? <Spinner /> : <CheckIcon />}</MarkerIcon>
      <MarkerContent className={running ? 'shimmer' : undefined}>{label}</MarkerContent>
    </Marker>
  );
}

/** 上下文压缩标签：官方 marker-demo 同款 separator 形态（居中标签 + 分隔线）。 */
function CompactionMarker({ label }: { label: string }) {
  return (
    <Marker className="olchat-enter" variant="separator">
      <MarkerContent className="text-xs">{label}</MarkerContent>
    </Marker>
  );
}

/** 思考行：与转译胶囊一模一样的流体圆点（ThinkingOrb）+ 扫光文案。 */
function WorkingRow({ label }: { label: string }) {
  return (
    <div className="olchat-enter flex items-center gap-2.5" role="status">
      <ThinkingOrb size={48} />
      <span className="shimmer text-xs font-medium">{label}</span>
    </div>
  );
}

function ApprovalCard({
  card,
  onDecide,
  t,
}: {
  card: ApprovalSegment;
  onDecide: (token: string, approved: boolean) => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  const decided = card.decision != null;
  return (
    <div className="olchat-enter flex flex-col gap-2 rounded-2xl border border-destructive/30 bg-destructive/5 p-3">
      <div className="text-[12.5px] font-semibold text-foreground">
        {t('lessComputer.approvalTitle')}
      </div>
      <code className="rounded-md bg-muted px-2 py-1.5 font-mono text-[11.5px] break-all text-foreground">
        {card.command}
      </code>
      <div className="text-[11.5px] text-muted-foreground">{card.reason}</div>
      {!decided && (
        <div className="rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1.5 text-[11px] leading-snug text-amber-700">
          {t('lessComputer.approvalRerunWarning')}
        </div>
      )}
      {decided ? (
        <div className="text-[11.5px] font-semibold text-muted-foreground">
          {card.decision === 'approved'
            ? t('lessComputer.approved')
            : t('lessComputer.denied')}
        </div>
      ) : (
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            className="flex-1"
            onMouseDown={event => event.stopPropagation()}
            onClick={() => onDecide(card.token, false)}
          >
            {t('lessComputer.deny')}
          </Button>
          <Button
            size="sm"
            className="flex-1"
            onMouseDown={event => event.stopPropagation()}
            onClick={() => onDecide(card.token, true)}
          >
            {t('lessComputer.approve')}
          </Button>
        </div>
      )}
    </div>
  );
}
