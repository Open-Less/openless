// QaPanel.tsx — 划词追问浮窗 v3（issue #118 v2）。
//
// 结构 = 官方 shadcn base 组件文档 message-scroller-demo **同款骨架**：
//   MessageScrollerProvider → Card（CardHeader 标题/副行/CardAction ✕ →
//   CardContent(p-0) 内 MessageScroller / Empty 空状态 → CardFooter 内
//   InputGroup 输入组）。组件源码 1:1 来自官方 registry（components/chat/ui/）。
//
// 「交流」形态（用户拍板）：官方 **Message** 带头像 —— 右侧用户消息挂 GitHub
// 头像（未登录 = GitHub 图标），左侧助手消息挂**旋转中的**胶囊思考头像
// （orbFeed 共享渲染源镜像）。
//
// 语音状态**只在输入区**表达（用户拍板：不进面板上部）：
//   录音 = 输入组绕圈红光；转译思考（问题还没落到对话里）= 绕圈黑光；
//   问题落定发出后光停，此时才在对话里出现助手思考行 —— 头像永远不先于
//   用户的话出现（修「AI 头像先出来、动画抽搐」）。
//
// 触发链路：
//   1) 用户按 Cmd+Shift+;（默认）→ 后端 toggle 浮窗可见性；显示时发
//      `qa:state { kind: "idle", messages: [] }` 与 `chat-panel:shown`（入场动画）。
//   2) 提问两条并存路径：文字（输入框 Enter → qa_submit_text）与语音
//      （麦克风按钮 / 听写键 → 录音 → ASR + LLM）。
//   3) 答案后可继续多轮追问，messages 累积。
//
// 关闭：Esc / ✕ / 再按 Cmd+Shift+; → qa_window_dismiss → 后端发
// `chat-panel:closing`（退场动画）→ 240ms 后隐藏窗口并清历史。

import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpIcon,
  CheckIcon,
  MessageCircleDashedIcon,
  MicIcon,
  SquareIcon,
  XIcon,
} from 'lucide-react';
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
import { Message, MessageAvatar, MessageContent } from '../components/chat/ui/message';
import { Bubble, BubbleContent } from '../components/chat/ui/bubble';
import { Button } from '../components/chat/ui/button';
import { OrbAvatar, UserAvatar, useGithubLogin } from '../components/chat/avatars';
import { AssistantMarkdown } from '../components/chat/markdown';
import { useChatPanelLifecycle } from '../components/chat/lifecycle';
import { cn } from '../components/chat/lib/utils';
import {
  chatPanelFocusKeyboard,
  confirmSelectionVoicePreview,
  getSelectionVoicePreview,
  isTauri,
  qaSetEditInstructionMode,
  qaSubmitText,
  qaToggleRecording,
  qaWindowDismiss,
  revertSelectionVoicePreview,
} from '../lib/ipc';
import { acceptQaSessionEvent, splitQaUserMessage } from '../lib/qaMessage';
import type { QaChatMessage, QaStatePayload } from '../lib/types';
import '../components/chat/chat.css';

const SELECTION_PREVIEW_MAX = 60;

type Status = 'idle' | 'recording' | 'thinking' | 'error';

interface QaPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
}

// 浏览器预览（vite dev，非 Tauri）：?window=qa&demo=1 注入两轮演示对话，方便调样式。
function getPreviewMessages(): QaChatMessage[] {
  if (isTauri || typeof window === 'undefined') return [];
  if (new URLSearchParams(window.location.search).get('demo') !== '1') return [];
  return [
    {
      role: 'user',
      content:
        '<selected_text>\nThe mitochondria is the powerhouse of the cell.\n</selected_text>\n\n# 我的问题\n这句话为什么会成为梗？',
    },
    {
      role: 'assistant',
      content:
        '这句话出自初中生物课本，因为**被反复要求背诵**而深入人心。它简短好记、几乎人人见过，于是在英文互联网上被当作“唯一记得的知识点”反复调侃——任何需要往里塞一句正经知识的场合都能用它。',
    },
    {
      role: 'user',
      content: '那更严谨的说法是什么？',
    },
    {
      role: 'assistant',
      content:
        '线粒体主要通过**氧化磷酸化**生成 ATP，是细胞的“能量工厂”没错，但：\n\n1. 它不是唯一的能量来源（糖酵解在细胞质里进行）\n2. 它还参与钙离子调控、凋亡信号等\n\n所以课本那句是简化版，方便记忆但不完整。',
    },
  ];
}

/** macOS movableByWindowBackground 拖动把手（header 区域整条可拖，普通箭头指针）。 */
const drag = { 'data-tauri-drag-region': true } as const;

export function QaPanel({ embedded = false, onRequestClose }: QaPanelProps = {}) {
  const { t } = useTranslation();
  const [messages, setMessages] = useState<QaChatMessage[]>(getPreviewMessages);
  const [status, setStatus] = useState<Status>('idle');
  const [errorMsg, setErrorMsg] = useState<string>('');
  const [selectionPreview, setSelectionPreview] = useState<string>('');
  const [composerText, setComposerText] = useState<string>('');
  /** 流式 LLM 答案：answer_delta 累积、answer 事件来时清空（最终内容已落到 messages）。 */
  const [streamingAnswer, setStreamingAnswer] = useState<string>('');
  const [editApplyAvailable, setEditApplyAvailable] = useState(false);
  const [editRevertAvailable, setEditRevertAvailable] = useState(false);
  const [editApplyBusy, setEditApplyBusy] = useState(false);
  const [editInstructionMode, setEditInstructionMode] = useState(false);
  const activeSessionIdRef = useRef<string | null>(null);
  const { enterEpoch, closing } = useChatPanelLifecycle();
  const tRef = useRef(t);
  tRef.current = t;
  const embeddedRef = useRef(embedded);
  embeddedRef.current = embedded;
  const onRequestCloseRef = useRef(onRequestClose);
  onRequestCloseRef.current = onRequestClose;

  // 新轮次信号：用户消息条数变化 = 新提问发出，也用它刷新 GitHub 头像。
  const userTurnCount = messages.filter(m => m.role === 'user').length;
  const githubLogin = useGithubLogin(userTurnCount);

  // ── 后端事件订阅（mount 时订阅一次，永不重订阅）──────────────────
  useEffect(() => {
    if (!isTauri) return;
    let unlistenState: (() => void) | undefined;
    let unlistenDismiss: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const stateHandle = await listen<QaStatePayload>('qa:state', event => {
          const payload = event.payload;
          const sessionEvent = acceptQaSessionEvent(activeSessionIdRef.current, payload);
          if (!sessionEvent.accepted) {
            return;
          }
          activeSessionIdRef.current = sessionEvent.sessionId;
          if (payload.messages) {
            setMessages(payload.messages);
          }
          if (typeof payload.edit_apply_available === 'boolean') {
            setEditApplyAvailable(payload.edit_apply_available);
          }
          if (typeof payload.edit_revert_available === 'boolean') {
            setEditRevertAvailable(payload.edit_revert_available);
          }
          if (typeof payload.edit_instruction_mode === 'boolean') {
            setEditInstructionMode(payload.edit_instruction_mode);
          }
          switch (payload.kind) {
            case 'idle':
              setStatus('idle');
              setSelectionPreview('');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'recording':
              setStatus('recording');
              setSelectionPreview(payload.selection_preview ?? '');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'loading':
              // ASR 在 finalize、user message 还没 push 的过渡帧。提前切到 thinking
              // 视图避免 UI 卡 recording 几百 ms 反馈缺失。详见 issue #161。
              setStatus('thinking');
              if (payload.selection_preview != null) {
                setSelectionPreview(payload.selection_preview);
              }
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'thinking':
              setStatus('thinking');
              if (payload.selection_preview != null) {
                setSelectionPreview(payload.selection_preview);
              }
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'answer_delta':
              // 流式增量。仍保持 thinking 状态——直到 answer 事件落定后才回 idle。
              if (payload.chunk) {
                setStreamingAnswer(prev => prev + payload.chunk);
              }
              break;
            case 'answer':
              setStatus('idle');
              setErrorMsg('');
              // messages 已被上面的 setMessages 落定，清掉流式 buffer 避免和最终气泡重影。
              setStreamingAnswer('');
              break;
            case 'error':
              setStatus('error');
              setErrorMsg(payload.error ?? tRef.current('qa.error'));
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
          }
        });
        const dismissHandle = await listen<unknown>('qa:dismiss', () => {
          activeSessionIdRef.current = null;
          setSelectionPreview('');
          setComposerText('');
          if (embeddedRef.current) {
            onRequestCloseRef.current?.();
          } else {
            void qaWindowDismiss();
          }
        });
        if (cancelled) {
          stateHandle();
          dismissHandle();
        } else {
          unlistenState = stateHandle;
          unlistenDismiss = dismissHandle;
        }
      } catch (error) {
        console.error('[QaPanel] listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      unlistenState?.();
      unlistenDismiss?.();
    };
  }, []);

  useEffect(() => {
    if (!closing) return;
    activeSessionIdRef.current = null;
    setMessages([]);
    setStatus('idle');
    setErrorMsg('');
    setStreamingAnswer('');
    setSelectionPreview('');
    setComposerText('');
    setEditInstructionMode(false);
  }, [closing]);

  // ── Esc 关闭 ────────────────────────────────────────────────────────
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        void qaWindowDismiss();
        onRequestClose?.();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onRequestClose]);

  const onClose = () => {
    void qaWindowDismiss();
    onRequestClose?.();
  };

  const onSubmitText = () => {
    const text = composerText.trim();
    if (!text || status === 'thinking' || status === 'recording') return;
    setComposerText('');
    void qaSubmitText(text).catch(error => {
      console.error('[QaPanel] qa_submit_text failed', error);
      setErrorMsg(error instanceof Error ? error.message : String(error));
      setStatus('error');
    });
  };

  const onToggleRecording = () => {
    if (status === 'thinking') return;
    void qaToggleRecording().catch(error => {
      console.error('[QaPanel] qa_toggle_recording failed', error);
    });
  };

  const onEditInstructionModeChange = (enabled: boolean) => {
    setEditInstructionMode(enabled);
    void qaSetEditInstructionMode(enabled).catch(error => {
      console.error('[QaPanel] qa_set_edit_instruction_mode failed', error);
    });
  };

  const onApplyEdit = async () => {
    if (!editApplyAvailable || editApplyBusy) return;
    setEditApplyBusy(true);
    setErrorMsg('');
    try {
      const qaSessionId = activeSessionIdRef.current;
      if (!qaSessionId) {
        throw new Error(t('qa.editApplyUnavailable'));
      }
      const preview = await getSelectionVoicePreview(qaSessionId);
      const text = preview?.text?.trim();
      if (!text) {
        throw new Error(t('qa.editApplyUnavailable'));
      }
      await confirmSelectionVoicePreview(text, qaSessionId);
      setEditApplyAvailable(false);
      setEditRevertAvailable(false);
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error));
      setStatus('error');
    } finally {
      setEditApplyBusy(false);
    }
  };

  const onRevertEdit = async () => {
    if (!editRevertAvailable || editApplyBusy) return;
    setEditApplyBusy(true);
    setErrorMsg('');
    try {
      const qaSessionId = activeSessionIdRef.current;
      if (!qaSessionId) {
        throw new Error(t('qa.editApplyUnavailable'));
      }
      await revertSelectionVoicePreview(qaSessionId);
      setEditRevertAvailable(false);
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error));
      setStatus('error');
    } finally {
      setEditApplyBusy(false);
    }
  };

  const lastRole = messages[messages.length - 1]?.role;
  // 问题是否已落进对话（转译完成 + 提交）。落定前头像不出现、黑光在输入框跑。
  const questionLanded = lastRole === 'user' || streamingAnswer.length > 0;
  // 输入组环形光：录音红光 → 转译思考黑光 → 问题落定即停（用户拍板的三段式）。
  const ring =
    status === 'recording'
      ? 'recording'
      : status === 'thinking' && !questionLanded
        ? 'thinking'
        : undefined;
  // 助手思考行：问题已在对话里、答案还没开始流出时才出现（头像不先于用户的话）。
  const thinkingRow = status === 'thinking' && !streamingAnswer && lastRole === 'user';
  const showEmpty =
    messages.length === 0 && !streamingAnswer && !thinkingRow && status !== 'error';

  // ── 官方 message-scroller-demo 同款骨架 ─────────────────────────────
  return (
    <MessageScrollerProvider
      autoScroll
      defaultScrollPosition="last-anchor"
      scrollPreviousItemPeek={18}
    >
      <Card
        key={enterEpoch}
        className={cn(
          'olchat-shell olchat-shell-in h-screen w-full gap-0',
          embedded && 'min-h-screen rounded-none shadow-none ring-0',
          closing && 'olchat-shell-out',
        )}
      >
        <CardHeader {...(embedded ? {} : drag)} className="gap-1 border-b">
          <CardTitle {...(embedded ? {} : drag)}>{t('qa.title')}</CardTitle>
          {/* 副行固定欢迎语：录音/思考状态不进面板上部（用户拍板），语音反馈全在输入区。 */}
          <CardDescription {...(embedded ? {} : drag)}>{t('qa.headerHint')}</CardDescription>
          <CardAction>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={onClose}
              onMouseDown={event => {
                event.preventDefault();
                event.stopPropagation();
              }}
              title={t('qa.closeTooltip')}
              aria-label={t('qa.closeTooltip')}
            >
              <XIcon />
            </Button>
          </CardAction>
        </CardHeader>
        <CardContent className="flex-1 overflow-hidden p-0">
          {showEmpty ? (
            <Empty className="h-full">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <MessageCircleDashedIcon />
                </EmptyMedia>
                <EmptyTitle>{t('qa.emptyTitle')}</EmptyTitle>
                <EmptyDescription>{t('qa.emptyDesc')}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <MessageScroller>
              <MessageScrollerViewport>
                <MessageScrollerContent
                  aria-busy={status === 'thinking' || undefined}
                  className="p-(--card-spacing)"
                >
                  {messages.map((message, i) => (
                    <MessageRow key={i} index={i} message={message} githubLogin={githubLogin} />
                  ))}
                  {streamingAnswer && (
                    <MessageScrollerItem messageId="streaming">
                      <Message>
                        <AiAvatar />
                        <MessageContent>
                          <AssistantMarkdown markdown={streamingAnswer} streaming />
                        </MessageContent>
                      </Message>
                    </MessageScrollerItem>
                  )}
                  {thinkingRow && (
                    <MessageScrollerItem messageId="thinking" className="olchat-enter">
                      <Message role="status">
                        <AiAvatar />
                        <MessageContent className="justify-center">
                          <span className="shimmer w-fit text-xs font-medium">
                            {t('qa.thinking')}
                          </span>
                        </MessageContent>
                      </Message>
                    </MessageScrollerItem>
                  )}
                  {status === 'error' && (
                    <MessageScrollerItem messageId="error" className="olchat-enter">
                      <Bubble variant="destructive">
                        <BubbleContent>
                          <div>{errorMsg}</div>
                          <div className="mt-1 text-[11.5px] opacity-70">
                            {t('qa.errorRetryHint')}
                          </div>
                        </BubbleContent>
                      </Bubble>
                    </MessageScrollerItem>
                  )}
                </MessageScrollerContent>
              </MessageScrollerViewport>
              <MessageScrollerButton aria-label={t('qa.jumpToLatest')} />
            </MessageScroller>
          )}
        </CardContent>
        <CardFooter className="flex-col gap-2">
          {status === 'recording' && selectionPreview && (
            <SelectionChip text={selectionPreview} t={t} />
          )}
          {editApplyAvailable && status === 'idle' && (
            <div className="flex w-full flex-col gap-2">
              {editRevertAvailable && (
                <Button
                  type="button"
                  variant="outline"
                  className="w-full"
                  disabled={editApplyBusy}
                  onClick={() => void onRevertEdit()}
                >
                  {t('qa.editRevertPrevious')}
                </Button>
              )}
              <Button
                type="button"
                className="w-full"
                disabled={editApplyBusy}
                onClick={() => void onApplyEdit()}
              >
                <CheckIcon />
                {t('qa.editApplyReplace')}
              </Button>
            </div>
          )}
          <Composer
            value={composerText}
            status={status}
            ring={ring}
            embedded={embedded}
            editInstructionMode={editInstructionMode}
            onEditInstructionModeChange={onEditInstructionModeChange}
            onChange={setComposerText}
            onSubmit={onSubmitText}
            onToggleRecording={onToggleRecording}
            t={t}
          />
        </CardFooter>
      </Card>
    </MessageScrollerProvider>
  );
}

// ── 底部输入区：官方 demo 同款 InputGroup，打字 + 语音两种形式完整 ────
//
// · 打字：Enter 走表单提交；IME 组合中的 Enter（选字确认）不触发（isComposing/
//   keyCode 229 守卫）。点进输入框时 chat_panel_focus_keyboard 让非激活面板
//   成为 key window（不激活 app，主窗口不动）。
// · 语音：麦克风按钮开/停录音；录音红光、转译黑光绕输入组一圈圈跑
//   （olchat-ring），输入框本体保持可见。
function Composer({
  value,
  status,
  ring,
  embedded,
  editInstructionMode,
  onEditInstructionModeChange,
  onChange,
  onSubmit,
  onToggleRecording,
  t,
}: {
  value: string;
  status: Status;
  ring: 'recording' | 'thinking' | undefined;
  embedded: boolean;
  editInstructionMode: boolean;
  onEditInstructionModeChange: (enabled: boolean) => void;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onToggleRecording: () => void;
  t: ReturnType<typeof useTranslation>['t'];
}) {
  // IME 组合期间的 Enter 是「选字确认」不是「发送」。keydown 里 isComposing
  // 已覆盖大部分场景，keyCode 229 兜底 WebKit 老行为。
  const composingRef = useRef(false);
  const recording = status === 'recording';
  const busy = status === 'thinking' || recording;

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
        onSubmit();
      }}
      className="w-full"
    >
      <InputGroup className="olchat-ring" data-ring={ring}>
        <InputGroupInput
          value={value}
          placeholder={t('qa.composerPlaceholder')}
          onChange={event => onChange(event.currentTarget.value)}
          onKeyDown={onKeyDown}
          onCompositionStart={() => {
            composingRef.current = true;
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
          }}
          onFocus={embedded ? undefined : () => void chatPanelFocusKeyboard()}
          onPointerDown={embedded ? undefined : () => void chatPanelFocusKeyboard()}
        />
        <InputGroupAddon align="block-end" className="pt-1">
          <label className="mr-auto flex cursor-pointer items-center gap-1.5 pl-1 text-[11.5px] text-muted-foreground select-none">
            <input
              type="checkbox"
              className="size-3.5 accent-foreground"
              checked={editInstructionMode}
              disabled={busy}
              onChange={event => onEditInstructionModeChange(event.currentTarget.checked)}
            />
            {t('qa.editInstructionMode')}
          </label>
          <InputGroupButton
            type="button"
            size="icon-sm"
            variant={recording ? 'destructive' : 'ghost'}
            disabled={status === 'thinking'}
            onClick={onToggleRecording}
            aria-pressed={recording}
            aria-label={recording ? t('qa.micStop') : t('qa.micLabel')}
            title={recording ? t('qa.micStop') : t('qa.micLabel')}
          >
            {recording ? <SquareIcon /> : <MicIcon />}
          </InputGroupButton>
          <InputGroupButton
            type="submit"
            variant="default"
            size="icon-sm"
            disabled={busy || !value.trim()}
          >
            <ArrowUpIcon />
            <span className="sr-only">{t('qa.composerSend')}</span>
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
    </form>
  );
}

// ── 子组件 ────────────────────────────────────────────────────────────

/** 助手头像：**旋转中的**胶囊思考动画（共享渲染源镜像，全部消息头像都在转）。 */
function AiAvatar() {
  return (
    <MessageAvatar className="size-8 bg-[#0b0b0f]">
      <OrbAvatar size={32} />
    </MessageAvatar>
  );
}

function MessageRow({
  index,
  message,
  githubLogin,
}: {
  index: number;
  message: QaChatMessage;
  githubLogin: string;
}) {
  if (message.role === 'user') {
    // 任意轮次都可能带选区信封：抽出问题单独显示，选区作引用块淡显在上。
    const { selection, question } = splitQaUserMessage(message);
    return (
      // 用户消息 = 新轮次锚点行（MessageScrollerItem scrollAnchor），右挂 GitHub 头像。
      <MessageScrollerItem messageId={`m${index}`} scrollAnchor className="olchat-enter">
        <Message align="end">
          <MessageAvatar>
            <UserAvatar login={githubLogin} />
          </MessageAvatar>
          <MessageContent>
            {selection && (
              <Bubble variant="muted" align="end" data-slot="selection">
                <BubbleContent className="text-xs text-muted-foreground italic">
                  “{truncate(selection, 120)}”
                </BubbleContent>
              </Bubble>
            )}
            <Bubble align="end">
              <BubbleContent>{question}</BubbleContent>
            </Bubble>
          </MessageContent>
        </Message>
      </MessageScrollerItem>
    );
  }
  return (
    <MessageScrollerItem messageId={`m${index}`} className="olchat-enter">
      <Message>
        <AiAvatar />
        <MessageContent>
          <AssistantMarkdown markdown={message.content} />
        </MessageContent>
      </Message>
    </MessageScrollerItem>
  );
}

/** 录音时的选区上下文条：贴在输入区上方，让用户看到「在追问哪段文字」。 */
function SelectionChip({ text, t }: { text: string; t: ReturnType<typeof useTranslation>['t'] }) {
  const truncated = useMemo(() => truncate(text, SELECTION_PREVIEW_MAX), [text]);
  return (
    <div className="olchat-enter flex w-full items-center rounded-xl bg-muted px-3 py-1.5 text-[11.5px] leading-normal">
      <span className="mr-1.5 shrink-0 text-muted-foreground">{t('qa.selectionPreview')}</span>
      <span className="truncate text-foreground/80">{truncated}</span>
    </div>
  );
}

function truncate(text: string, max: number): string {
  if (text.length <= max) return text;
  return `${text.slice(0, max)}…`;
}
