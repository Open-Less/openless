// Less Computer desktop workspace. The event replay and voice projection remain
// authoritative; history/multi-session placeholders never invent executable state.
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpIcon,
  CheckIcon,
  ChevronRightIcon,
  HistoryIcon,
  LayersIcon,
  Maximize2Icon,
  MicIcon,
  MinusIcon,
  PlusIcon,
  ShieldCheckIcon,
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
import { VoiceWaveform } from '../components/chat/VoiceWaveform';
import { AssistantMarkdown } from '../components/chat/markdown';
import { useChatPanelLifecycle } from '../components/chat/lifecycle';
import { GithubLoginModal } from '../components/GithubLoginModal';
import {
  chatPanelFocusKeyboard,
  getSettings,
  isTauri,
  lessComputerApprove,
  lessComputerSubmitText,
  lessComputerSync,
  lessComputerWindowDismiss,
  marketplaceAuthStatus,
} from '../lib/ipc';
import { reconcileLessComputerReplay, reduceLessComputerVoice } from '../lib/lessComputerReplay';
import type {
  CodingAgentProviderId,
  LessComputerEvent,
  LessComputerVoiceEvent,
  UserPreferences,
} from '../lib/types';
import { groupToolActivities, toolActivityCategory } from '../lib/lessComputerToolActivity';
import './less-computer-panel.css';

type Translate = ReturnType<typeof useTranslation>['t'];
const AGENTS: { id: CodingAgentProviderId; name: string }[] = [
  { id: 'claude-code-cli', name: 'Claude Code' },
  { id: 'opencode-cli', name: 'OpenCode' },
  { id: 'codex-cli', name: 'Codex' },
  { id: 'dsh-cli', name: 'dsh' },
];

type RunStatus = 'idle' | 'working' | 'done' | 'error' | 'cancelled';

interface TextSegment {
  kind: 'text';
  content: string;
}

interface ToolSegment {
  kind: 'tool';
  name: string;
  /** 后端没有工具结束事件：下一个事件到达时仅停止活动指示，不推断工具成功。 */
  running: boolean;
}

interface ApprovalSegment {
  kind: 'approval';
  token: string;
  command: string;
  reason: string;
  /** Only confirmed IPC results become decisions. Pending requests remain undecided. */
  pending?: boolean;
  failed?: boolean;
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

/** 把流里还在扫光的工具行停下来（下一个事件到达仅停止工具活动指示）。 */
function settleRunningTools(segments: Segment[]): Segment[] {
  if (!segments.some((s) => s.kind === 'tool' && s.running)) return segments;
  return segments.map((s) => (s.kind === 'tool' && s.running ? { ...s, running: false } : s));
}

// Keep the replay watermark across StrictMode/HMR effect remounts. A whole
// WebView reload resets both state and watermark so the backend can replay it.
let lcAppliedSeq = 0;

export function LessComputerPanel() {
  const { t } = useTranslation();
  const [turns, setTurns] = useState<Turn[]>([]);
  const [voice, setVoice] = useState<LessComputerVoiceEvent | null>(null);
  const [sessionSeq, setSessionSeq] = useState(0);
  const [provider, setProvider] = useState<CodingAgentProviderId | null>(null);
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [windowError, setWindowError] = useState(false);
  const turnEpoch = useRef(0);
  const approvalRequests = useRef(new Map<string, symbol>());
  const { enterEpoch, closing } = useChatPanelLifecycle();

  // Read actual configuration and account status; browser previews stay unavailable.
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const current = ++revision;
      const results = await Promise.allSettled([getSettings(), marketplaceAuthStatus()]);
      if (cancelled || current !== revision) return;
      const [settings, account] = results;
      setProvider(settings.status === 'fulfilled' ? settings.value.codingAgentProvider : null);
      setSignedIn(account.status === 'fulfilled' ? account.value.signedIn : null);
    };
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen<UserPreferences>('prefs:changed', (event) => {
          revision += 1;
          setProvider(event.payload.codingAgentProvider);
        });
        if (cancelled) {
          handle();
          return;
        }
        unlisten = handle;
      } catch {
        /* focus refresh still works when the optional subscription fails */
      }
      if (!cancelled) void refresh();
    })();
    window.addEventListener('focus', refresh);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener('focus', refresh);
    };
  }, [loginOpen]);

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
        const handle = await listen<LessComputerEvent>('less-computer:event', (event) => {
          if (synced) applyDeduped(event.payload);
          else pending.push(event.payload);
        });
        if (cancelled) {
          handle();
          return;
        }
        unlisten = handle;
        const replay = await lessComputerSync(lcAppliedSeq).catch((error) => {
          console.error('[LessComputer] sync failed', error);
          return {
            events: [] as LessComputerEvent[],
            latestSequence: lcAppliedSeq,
            truncated: false,
            voiceState: undefined,
          };
        });
        if (cancelled) return;
        const reconciled = reconcileLessComputerReplay(lcAppliedSeq, replay, pending);
        if (reconciled.reset) {
          turnEpoch.current += 1;
          approvalRequests.current.clear();
          setTurns([]);
          setVoice(null);
        }
        // 投影有自己的原始seq，不推进聊天流水位；读取投影期间到达的普通事件仍需应用。
        if (replay.voiceState) {
          const snapshot = replay.voiceState;
          setVoice((previous) => reduceLessComputerVoice(previous, snapshot, true));
        }
        for (const ev of reconciled.events) applyEvent(ev);
        lcAppliedSeq = reconciled.latestAppliedSequence;
        synced = true;
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
      case 'voice_state':
        setVoice((previous) => reduceLessComputerVoice(previous, ev));
        break;
      case 'user': {
        turnEpoch.current += 1;
        approvalRequests.current.clear();
        // 一轮新对话。fresh=true（后端无可续会话→新会话）则清空历史重开；否则追加为后续轮次。
        setTurns((prev) => (ev.fresh ? [emptyTurn(ev.text)] : [...prev, emptyTurn(ev.text)]));
        if (ev.fresh) setSessionSeq((seq) => seq + 1);
        break;
      }
      case 'started':
        setTurns((prev) => updateLastTurn(prev, (tn) => ({ ...tn, status: 'working' })));
        break;
      case 'delta':
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => {
            const segments = settleRunningTools(tn.segments);
            const last = segments[segments.length - 1];
            if (last?.kind === 'text') {
              return {
                ...tn,
                status: 'working',
                segments: [...segments.slice(0, -1), { ...last, content: last.content + ev.text }],
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
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => ({
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
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => ({
            ...tn,
            segments: [...settleRunningTools(tn.segments), { kind: 'compaction' }],
          })),
        );
        break;
      case 'approval':
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => ({
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
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => {
            let segments = settleRunningTools(tn.segments);
            // 正常情况最终文本已通过 delta 流出；只有整轮没有任何文本时才用
            // completed 的成品兜底（否则会把穿插的工具行冲掉）。
            if (ev.text && !segments.some((s) => s.kind === 'text')) {
              segments = [...segments, { kind: 'text', content: ev.text }];
            }
            return { ...tn, segments, costUsd: ev.costUsd ?? null, status: 'done' };
          }),
        );
        break;
      case 'error':
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => ({
            ...tn,
            segments: settleRunningTools(tn.segments),
            errorMsg: ev.message,
            status: 'error',
          })),
        );
        break;
      case 'cancelled':
        setTurns((prev) =>
          updateLastTurn(prev, (tn) => ({
            ...tn,
            segments: settleRunningTools(tn.segments),
            status: 'cancelled',
          })),
        );
        break;
    }
  };

  const onApproval = async (token: string, approved: boolean) => {
    const currentTurn = turns[turns.length - 1];
    if (!isTauri || currentTurn?.status !== 'working' || approvalRequests.current.has(token))
      return;
    const card = currentTurn.segments.find(
      (segment) => segment.kind === 'approval' && segment.token === token,
    );
    if (!card || card.kind !== 'approval' || card.decision) return;
    const request = Symbol(token);
    const epoch = turnEpoch.current;
    approvalRequests.current.set(token, request);
    const update = (patch: Partial<ApprovalSegment>) =>
      setTurns((previous) =>
        previous.map((turn) => ({
          ...turn,
          segments: turn.segments.map((segment) =>
            segment.kind === 'approval' && segment.token === token
              ? { ...segment, ...patch }
              : segment,
          ),
        })),
      );
    update({ pending: true, failed: false });
    try {
      await lessComputerApprove(token, approved);
      if (turnEpoch.current === epoch)
        update({ pending: false, decision: approved ? 'approved' : 'denied' });
    } catch {
      if (turnEpoch.current === epoch) update({ pending: false, failed: true });
    } finally {
      if (approvalRequests.current.get(token) === request) approvalRequests.current.delete(token);
    }
  };

  const windowAction = async (action: 'hide' | 'minimize' | 'maximize') => {
    if (!isTauri) return;
    setWindowError(false);
    try {
      if (action === 'hide') await lessComputerWindowDismiss();
      else {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const nativeWindow = getCurrentWindow();
        if (action === 'minimize') await nativeWindow.minimize();
        else await nativeWindow.toggleMaximize();
      }
    } catch {
      setWindowError(true);
    }
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      if (loginOpen) {
        event.stopPropagation();
        setLoginOpen(false);
      } else void windowAction('hide');
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [loginOpen]);

  const working = turns.some((turn) => turn.status === 'working');
  const latestTurn = turns[turns.length - 1];
  const status = runStatusLabel(latestTurn, t);

  return (
    <>
      <div
        className={`lc-desktop${closing ? ' is-closing' : ''}`}
        key={`${sessionSeq}-${enterEpoch}`}
      >
        <header className="lc-topbar" data-tauri-drag-region>
          <div className="lc-window-controls">
            <button
              className="lc-window-close"
              type="button"
              disabled={!isTauri}
              aria-label={t('lessComputer.closeTooltip')}
              title={t('lessComputer.closeTooltip')}
              onClick={() => void windowAction('hide')}
            >
              <XIcon />
            </button>
            <button
              className="lc-window-minimize"
              type="button"
              disabled={!isTauri}
              aria-label={t('lessComputer.desktop.minimize')}
              title={t('lessComputer.desktop.minimize')}
              onClick={() => void windowAction('minimize')}
            >
              <MinusIcon />
            </button>
            <button
              className="lc-window-maximize"
              type="button"
              disabled={!isTauri}
              aria-label={t('lessComputer.desktop.maximize')}
              title={t('lessComputer.desktop.maximize')}
              onClick={() => void windowAction('maximize')}
            >
              <Maximize2Icon />
            </button>
          </div>
          <div className="lc-session-title" data-tauri-drag-region>
            <span className="lc-session-dot" />
            <span data-tauri-drag-region>{t('lessComputer.desktop.currentSession')}</span>
          </div>
        </header>
        <aside className="lc-sidebar" aria-label={t('lessComputer.desktop.agents')}>
          <div className="lc-sidebar-scroll">
            <div className="lc-section-label">{t('lessComputer.desktop.agents')}</div>
            <div className="lc-agent-list">
              {AGENTS.map((agent) => (
                <div
                  key={agent.id}
                  className={`lc-agent${provider === agent.id ? ' is-configured' : ''}`}
                  title={agent.name}
                >
                  <div className="lc-agent-name">
                    <strong>{agent.name}</strong>
                    <span>
                      {provider === agent.id
                        ? t('lessComputer.desktop.configured')
                        : t('lessComputer.desktop.agentSettings')}
                    </span>
                  </div>
                  <div className="lc-agent-actions">
                    <button
                      type="button"
                      disabled
                      aria-label={`${agent.name} · ${t('lessComputer.desktop.historyUnavailable')}`}
                      title={t('lessComputer.desktop.historyUnavailable')}
                    >
                      <HistoryIcon />
                    </button>
                    <button
                      type="button"
                      disabled
                      aria-label={`${agent.name} · ${t('lessComputer.desktop.sessionUnavailable')}`}
                      title={t('lessComputer.desktop.sessionUnavailable')}
                    >
                      <PlusIcon />
                    </button>
                  </div>
                </div>
              ))}
            </div>
            <p className="lc-sidebar-note">{t('lessComputer.desktop.agentHint')}</p>
            <div className="lc-section-label lc-task-label">
              {t('lessComputer.desktop.workspace')}
            </div>
            <button className="lc-placeholder" type="button" disabled>
              <LayersIcon />
              <span>
                {t('lessComputer.desktop.multitask')}
                <small>{t('lessComputer.desktop.unavailable')}</small>
              </span>
              <PlusIcon />
            </button>
            <p className="lc-sidebar-note">{t('lessComputer.desktop.historyHint')}</p>
          </div>
          <button
            className="lc-github"
            type="button"
            disabled={!isTauri || signedIn === true}
            onClick={() => setLoginOpen(true)}
          >
            <GithubMark />
            <span>
              <strong>GitHub</strong>
              <small>
                {signedIn === true
                  ? t('lessComputer.desktop.signedIn')
                  : t('lessComputer.desktop.signIn')}
              </small>
            </span>
            {signedIn === true ? <CheckIcon /> : <ChevronRightIcon />}
          </button>
        </aside>

        <main className="lc-conversation" aria-label={t('lessComputer.desktop.currentSession')}>
          {windowError && (
            <p className="lc-notice" role="alert">
              {t('lessComputer.desktop.windowError')}
            </p>
          )}
          <div className="lc-conversation-heading">
            <h1>{t('lessComputer.title')}</h1>
            <span className={`lc-run-status is-${latestTurn?.status ?? 'idle'}`} role="status">
              <span />
              {status}
            </span>
          </div>
          <div className="lc-message-area">
            <MessageScrollerProvider
              autoScroll
              defaultScrollPosition="last-anchor"
              scrollPreviousItemPeek={18}
            >
              {turns.length === 0 ? (
                <div className="lc-empty">
                  <h2>{t('lessComputer.subtitle')}</h2>
                  <p>{t('lessComputer.desktop.emptyHint')}</p>
                  <span className="lc-empty-label">
                    <MicIcon />
                    {t('lessComputer.desktop.voiceHint')}
                  </span>
                </div>
              ) : (
                <MessageScroller>
                  <MessageScrollerViewport>
                    <MessageScrollerContent
                      aria-busy={working || undefined}
                      className="lc-messages"
                    >
                      {turns.map((turn, index) => (
                        <TurnView
                          key={index}
                          index={index}
                          turn={turn}
                          actionable={index === turns.length - 1}
                          onApproval={onApproval}
                          t={t}
                        />
                      ))}
                    </MessageScrollerContent>
                  </MessageScrollerViewport>
                  <MessageScrollerButton
                    className="lc-jump"
                    aria-label={t('lessComputer.jumpToLatest')}
                  />
                </MessageScroller>
              )}
            </MessageScrollerProvider>
          </div>
          <Composer working={working} voice={voice} t={t} />
        </main>
      </div>
      {loginOpen && isTauri && (
        <GithubLoginModal
          onClose={() => setLoginOpen(false)}
          onSuccess={() => {
            setSignedIn(true);
            setLoginOpen(false);
          }}
        />
      )}
    </>
  );
}

function runStatusLabel(turn: Turn | undefined, t: Translate): string {
  if (!turn) return t('lessComputer.desktop.idle');
  if (
    turn.status === 'working' &&
    turn.segments.some((segment) => segment.kind === 'approval' && !segment.decision)
  )
    return t('lessComputer.desktop.waitingApproval');
  if (turn.status === 'working') {
    const active = turn.segments.find((segment) => segment.kind === 'tool' && segment.running);
    return active?.kind === 'tool'
      ? t(`lessComputer.activity.${toolActivityCategory(active.name)}Running`)
      : t('lessComputer.working');
  }
  if (turn.status === 'done') return t('lessComputer.done');
  if (turn.status === 'cancelled') return t('common.cancelled');
  if (turn.status === 'error') return t('lessComputer.error');
  return t('lessComputer.desktop.idle');
}

function voiceTime(elapsedMs: number): string {
  const seconds = Math.floor(Math.max(0, Number.isFinite(elapsedMs) ? elapsedMs : 0) / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function Composer({
  working,
  voice,
  t,
}: {
  working: boolean;
  voice: LessComputerVoiceEvent | null;
  t: Translate;
}) {
  const [text, setText] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [failed, setFailed] = useState(false);
  const submittingRef = useRef(false);
  const composingRef = useRef(false);
  const speaking = voice !== null && voice.phase !== 'idle';
  const busy = working || speaking || submitting;
  const voiceLabel =
    voice?.phase === 'recording'
      ? t('overview.inAppDictation.recording')
      : voice?.phase === 'starting'
        ? t('common.loading')
        : t('overview.inAppDictation.processing');
  const send = async () => {
    const trimmed = text.trim();
    if (!isTauri || !trimmed || busy || submittingRef.current || composingRef.current) return;
    submittingRef.current = true;
    setSubmitting(true);
    setFailed(false);
    try {
      await lessComputerSubmitText(trimmed);
      // Keep a draft typed while the IPC was pending. The actual user event owns the chat.
      setText((current) => (current === text ? '' : current));
    } catch {
      setFailed(true);
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== 'Enter' || event.shiftKey) return;
    event.preventDefault();
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return;
    void send();
  };
  return (
    <div className="lc-composer-wrap">
      <form
        className={`lc-composer${speaking ? ' is-voice' : ''}`}
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <div className="lc-input-stage">
          <textarea
            className="lc-text-input"
            rows={2}
            value={text}
            disabled={!isTauri || speaking}
            placeholder={t('lessComputer.inputPlaceholder')}
            aria-label={t('lessComputer.inputPlaceholder')}
            onChange={(event) => setText(event.currentTarget.value)}
            onKeyDown={onKeyDown}
            onCompositionStart={() => {
              composingRef.current = true;
            }}
            onCompositionEnd={() => {
              composingRef.current = false;
            }}
            onFocus={() => {
              if (isTauri) void chatPanelFocusKeyboard().catch(() => undefined);
            }}
            onPointerDown={() => {
              if (isTauri) void chatPanelFocusKeyboard().catch(() => undefined);
            }}
          />
          <div className="lc-voice-stage" aria-hidden={!speaking}>
            {speaking && voice && (
              <>
                <VoiceWaveform
                  level={voice.phase === 'recording' ? voice.level : 0}
                  processing={voice.phase !== 'recording'}
                  label={voiceLabel}
                />
                <time>{voiceTime(voice.elapsedMs)}</time>
              </>
            )}
          </div>
        </div>
        <div className="lc-composer-footer">
          <span>
            <MicIcon />
            {t('lessComputer.desktop.voiceHint')}
          </span>
          <button
            className="lc-send"
            type="submit"
            disabled={!isTauri || busy || !text.trim()}
            aria-label={t('lessComputer.send')}
            title={t('lessComputer.send')}
          >
            <ArrowUpIcon />
          </button>
        </div>
      </form>
      <div className="lc-composer-caption">
        {failed ? (
          <span role="alert">{t('lessComputer.desktop.sendError')}</span>
        ) : !isTauri ? (
          t('lessComputer.desktop.browserUnavailable')
        ) : working ? (
          t('lessComputer.desktop.cancelHint')
        ) : (
          t('lessComputer.desktop.inputHint')
        )}
      </div>
    </div>
  );
}

function TurnView({
  index,
  turn,
  actionable,
  onApproval,
  t,
}: {
  index: number;
  turn: Turn;
  actionable: boolean;
  onApproval: (token: string, approved: boolean) => void;
  t: Translate;
}) {
  const hasUser = turn.user.trim().length > 0;
  const lastSegment = turn.segments[turn.segments.length - 1];
  const waiting =
    turn.status === 'working' &&
    (turn.segments.length === 0 ||
      (lastSegment?.kind === 'approval' && lastSegment.decision != null));
  return (
    <>
      {hasUser && (
        <MessageScrollerItem messageId={`t${index}-user`} scrollAnchor>
          <div className="lc-user-message">
            <span>{t('lessComputer.you')}</span>
            <p>{turn.user}</p>
          </div>
        </MessageScrollerItem>
      )}
      <MessageScrollerItem messageId={`t${index}-assistant`} scrollAnchor={!hasUser}>
        <div className="lc-assistant-message">
          <div className="lc-assistant-label">Less Computer</div>
          {turn.segments.map((segment, i) => {
            if (segment.kind === 'text')
              return (
                <div className="lc-answer" key={`s${i}`}>
                  <AssistantMarkdown
                    markdown={segment.content}
                    streaming={turn.status === 'working' && i === turn.segments.length - 1}
                  />
                </div>
              );
            if (segment.kind === 'tool') {
              if (turn.segments[i - 1]?.kind === 'tool') return null;
              const tools: ToolSegment[] = [];
              let end = i;
              while (end < turn.segments.length) {
                const candidate = turn.segments[end];
                if (candidate.kind !== 'tool') break;
                tools.push(candidate);
                end += 1;
              }
              return (
                <ToolProcess
                  key={`s${i}`}
                  tools={tools}
                  working={turn.status === 'working'}
                  interrupted={
                    (turn.status === 'error' || turn.status === 'cancelled') &&
                    end === turn.segments.length
                  }
                  t={t}
                />
              );
            }
            if (segment.kind === 'compaction')
              return (
                <div className="lc-compaction" key={`s${i}`}>
                  <LayersIcon />
                  {t('lessComputer.compaction')}
                </div>
              );
            return (
              <ApprovalCard
                key={`${segment.token}-${i}`}
                card={segment}
                actionable={actionable && turn.status === 'working'}
                onDecide={onApproval}
                t={t}
              />
            );
          })}
          {waiting && (
            <div className="lc-thinking" role="status">
              <span className="lc-active-dot" />
              {t('lessComputer.working')}
            </div>
          )}
          {turn.status === 'error' && (
            <p className="lc-run-error" role="alert">
              {turn.errorMsg || t('lessComputer.error')}
            </p>
          )}
          {turn.status === 'cancelled' && (
            <span className="lc-turn-footnote">{t('common.cancelled')}</span>
          )}
          {turn.status === 'done' && (
            <div className="lc-turn-footnote">
              <CheckIcon />
              {t('lessComputer.done')}
              {turn.costUsd != null && (
                <span>{t('lessComputer.cost', { cost: turn.costUsd.toFixed(3) })}</span>
              )}
            </div>
          )}
        </div>
      </MessageScrollerItem>
    </>
  );
}

function ToolProcess({
  tools,
  working,
  interrupted,
  t,
}: {
  tools: ToolSegment[];
  working: boolean;
  interrupted: boolean;
  t: Translate;
}) {
  const groups = groupToolActivities(tools, working, interrupted);
  const active = groups.find((group) => group.state === 'active');
  return (
    <details className="lc-tool-process">
      <summary>
        <ChevronRightIcon className="lc-process-chevron" />
        <span className={`lc-process-label${active ? ' is-running' : ''}`}>
          {active
            ? t(`lessComputer.activity.${active.category}Running`)
            : t('lessComputer.activity.process')}
        </span>
        <span className="lc-process-count">
          {t('lessComputer.activity.count', { count: tools.length })}
        </span>
        {!active && (
          <span className="lc-process-result">
            {interrupted ? t('lessComputer.activity.stopped') : t('lessComputer.activity.finished')}
          </span>
        )}
      </summary>
      <ol className="lc-process-steps">
        {groups.map((group, index) => (
          <li key={index} className={`lc-process-step is-${group.state}`}>
            <span className="lc-process-marker" aria-hidden="true">
              {group.state === 'active' ? (
                <span />
              ) : group.state === 'stopped' ? (
                <MinusIcon />
              ) : (
                <CheckIcon />
              )}
            </span>
            <div className="lc-process-step-content">
              <div className="lc-process-phase-row">
                <span
                  className={`lc-process-phase${group.state === 'active' ? ' is-running' : ''}`}
                >
                  {t(
                    `lessComputer.activity.${group.category}${group.state === 'active' ? 'Running' : ''}`,
                  )}
                </span>
                {group.state !== 'active' && (
                  <span className="lc-process-result">
                    {t(
                      `lessComputer.activity.${group.state === 'stopped' ? 'stopped' : 'finished'}`,
                    )}
                  </span>
                )}
              </div>
              <ul className="lc-process-tools">
                {group.names.map((tool, toolIndex) => (
                  <li key={toolIndex}>
                    <span>{tool.name}</span>
                    {tool.count > 1 && <span className="lc-tool-count">×{tool.count}</span>}
                  </li>
                ))}
              </ul>
            </div>
          </li>
        ))}
      </ol>
    </details>
  );
}

function ApprovalCard({
  card,
  actionable,
  onDecide,
  t,
}: {
  card: ApprovalSegment;
  actionable: boolean;
  onDecide: (token: string, approved: boolean) => void;
  t: Translate;
}) {
  return (
    <section className="lc-approval">
      <div className="lc-approval-title">
        <ShieldCheckIcon />
        <strong>{t('lessComputer.approvalTitle')}</strong>
      </div>
      <pre>{card.command}</pre>
      {card.reason && <p>{card.reason}</p>}
      <p className="lc-approval-warning">{t('lessComputer.approvalRerunWarning')}</p>
      {card.decision ? (
        <span className="lc-approval-result">
          <CheckIcon />
          {t(`lessComputer.desktop.${card.decision}Submitted`)}
        </span>
      ) : (
        <>
          <div className="lc-approval-actions">
            <button
              type="button"
              disabled={!isTauri || !actionable || card.pending}
              onClick={() => onDecide(card.token, false)}
            >
              {t('lessComputer.deny')}
            </button>
            <button
              className="lc-primary-button"
              type="button"
              disabled={!isTauri || !actionable || card.pending}
              onClick={() => onDecide(card.token, true)}
            >
              {card.pending
                ? t('lessComputer.desktop.submittingApproval')
                : t('lessComputer.approve')}
            </button>
          </div>
          {card.failed && (
            <p className="lc-run-error" role="alert">
              {t('lessComputer.desktop.approvalError')}
            </p>
          )}
          {!actionable && (
            <p className="lc-turn-footnote">{t('lessComputer.desktop.approvalExpired')}</p>
          )}
        </>
      )}
    </section>
  );
}

function GithubMark() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}
