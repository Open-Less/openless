// Less Computer desktop workspace. The event replay and voice projection remain
// authoritative; history/multi-session placeholders never invent executable state.
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpIcon,
  AudioLinesIcon,
  CheckIcon,
  ChevronRightIcon,
  CircleAlertIcon,
  FileTextIcon,
  GlobeIcon,
  LayersIcon,
  Maximize2Icon,
  MicIcon,
  MinusIcon,
  PanelRightOpenIcon,
  PencilLineIcon,
  SearchIcon,
  ShieldCheckIcon,
  SquareIcon,
  SquarePenIcon,
  TerminalIcon,
  WrenchIcon,
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
import { LiveWaveform } from '../components/chat/LiveWaveform';
import { AssistantMarkdown } from '../components/chat/markdown';
import { useChatPanelLifecycle } from '../components/chat/lifecycle';
import { GithubLoginModal } from '../components/GithubLoginModal';
import { Tooltip } from '../components/Tooltip';
import {
  chatPanelFocusKeyboard,
  getSettings,
  isTauri,
  lessComputerApprove,
  lessComputerSubmitText,
  lessComputerSync,
  lessComputerTaskCancel,
  lessComputerVoiceCancel,
  lessComputerVoiceStart,
  lessComputerVoiceStop,
  lessComputerWindowDismiss,
  marketplaceAuthStatus,
} from '../lib/ipc';
import { reconcileLessComputerReplay, reduceLessComputerVoice } from '../lib/lessComputerReplay';
import {
  claimDictationResult,
  mergeDictation,
  transcriptTail,
  voiceHintKey,
} from '../lib/lessComputerComposer';
import { formatComboLabel } from '../lib/hotkey';
import { applyThemeFromPreference } from '../lib/themeMode';
import { useExitMount } from '../lib/useExitMount';
import type {
  CodingAgentProviderId,
  HotkeyMode,
  LessComputerEvent,
  LessComputerVoiceEvent,
  LessComputerVoiceMode,
  UserPreferences,
} from '../lib/types';
import { groupToolActivities, toolActivityCategory } from '../lib/lessComputerToolActivity';
import { AgentAvatar, CopyAction, LessComputerInspector, type RunTone } from './LessComputerChrome';
import './less-computer-panel.css';

type Translate = ReturnType<typeof useTranslation>['t'];
const AGENTS: { id: CodingAgentProviderId; name: string }[] = [
  { id: 'claude-code-cli', name: 'Claude Code' },
  { id: 'opencode-cli', name: 'OpenCode' },
  { id: 'codex-cli', name: 'Codex' },
  { id: 'dsh-cli', name: 'dsh' },
];

const CATEGORY_ICONS = {
  search: SearchIcon,
  read: FileTextIcon,
  command: TerminalIcon,
  edit: PencilLineIcon,
  web: GlobeIcon,
  other: WrenchIcon,
};

const INSPECTOR_KEY = 'ol.lc.inspector';
const NARROW_QUERY = '(max-width: 899px)';
const LOGIN_EXIT_MS = 180;
const MAX_INPUT_HEIGHT = 168;

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

interface VoiceShortcut {
  label: string;
  mode: HotkeyMode;
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

function hasFinishedLastTurn(turns: Turn[]): boolean {
  const last = turns[turns.length - 1];
  return last !== undefined && last.status !== 'working';
}

/** 把流里还在扫光的工具行停下来（下一个事件到达仅停止工具活动指示）。 */
function settleRunningTools(segments: Segment[]): Segment[] {
  if (!segments.some((s) => s.kind === 'tool' && s.running)) return segments;
  return segments.map((s) => (s.kind === 'tool' && s.running ? { ...s, running: false } : s));
}

function readInspectorPreference(): boolean {
  try {
    return window.localStorage?.getItem(INSPECTOR_KEY) !== '0';
  } catch {
    return true;
  }
}

function writeInspectorPreference(open: boolean) {
  try {
    window.localStorage?.setItem(INSPECTOR_KEY, open ? '1' : '0');
  } catch {
    /* the in-memory preference still applies for this window */
  }
}

function matchesNarrow(): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia(NARROW_QUERY).matches;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

function voiceShortcutFrom(preferences: UserPreferences): VoiceShortcut | null {
  const binding = preferences.codingAgentVoiceHotkey;
  if (!binding) return null;
  return { label: formatComboLabel(binding), mode: preferences.hotkey?.mode ?? 'hold' };
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
  const [voiceShortcut, setVoiceShortcut] = useState<VoiceShortcut | null>(null);
  const [signedIn, setSignedIn] = useState<boolean | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [windowError, setWindowError] = useState(false);
  const [inspectorPreference, setInspectorPreference] = useState(readInspectorPreference);
  const [narrow, setNarrow] = useState(matchesNarrow);
  const [inspectorOverlay, setInspectorOverlay] = useState(false);
  const turnEpoch = useRef(0);
  const approvalRequests = useRef(new Map<string, symbol>());
  const shellRef = useRef<HTMLDivElement | null>(null);
  const closedRef = useRef(false);
  const { enterEpoch, closing } = useChatPanelLifecycle();
  const login = useExitMount(loginOpen, LOGIN_EXIT_MS);

  // Read actual configuration and account status; browser previews stay unavailable.
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    const applyPreferences = (preferences: UserPreferences) => {
      setProvider(preferences.codingAgentProvider);
      setVoiceShortcut(voiceShortcutFrom(preferences));
      if (preferences.themeMode) applyThemeFromPreference(preferences.themeMode);
    };
    const refresh = async () => {
      const current = ++revision;
      const results = await Promise.allSettled([getSettings(), marketplaceAuthStatus()]);
      if (cancelled || current !== revision) return;
      const [settings, account] = results;
      if (settings.status === 'fulfilled') applyPreferences(settings.value);
      else {
        setProvider(null);
        setVoiceShortcut(null);
      }
      setSignedIn(account.status === 'fulfilled' ? account.value.signedIn : null);
    };
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen<UserPreferences>('prefs:changed', (event) => {
          revision += 1;
          applyPreferences(event.payload);
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

  // Narrow windows show the inspector as an overlay that starts closed.
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const media = window.matchMedia(NARROW_QUERY);
    const onChange = () => {
      setNarrow(media.matches);
      setInspectorOverlay(false);
    };
    media.addEventListener('change', onChange);
    return () => media.removeEventListener('change', onChange);
  }, []);

  // The WebView is reused across show/hide. Replay only the entrance motion so
  // drafts, scroll position and inspector state survive reopening. Text and
  // voice turns also call the native show path while the panel is visible;
  // those must not flash the window, so only a preceding close replays it.
  useEffect(() => {
    if (closing) closedRef.current = true;
  }, [closing]);
  useEffect(() => {
    if (enterEpoch === 0 || !closedRef.current) return;
    closedRef.current = false;
    if (prefersReducedMotion()) return;
    const shell = shellRef.current;
    if (!shell || typeof shell.animate !== 'function') return;
    shell.animate(
      [
        { opacity: 0, transform: 'translateY(6px) scale(0.985)' },
        { opacity: 1, transform: 'none' },
      ],
      { duration: 240, easing: 'cubic-bezier(0.16, 1, 0.3, 1)' },
    );
  }, [enterEpoch]);

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
          // A capture that fails before any turn starts must not relabel the
          // previous finished answer; it gets its own error row instead.
          hasFinishedLastTurn(prev)
            ? [...prev, { ...emptyTurn(''), status: 'error', errorMsg: ev.message }]
            : updateLastTurn(prev, (tn) => ({
                ...tn,
                segments: settleRunningTools(tn.segments),
                errorMsg: ev.message,
                status: 'error',
              })),
        );
        break;
      case 'cancelled':
        setTurns((prev) =>
          // Abandoning a recording cancels only the capture, not a finished turn.
          hasFinishedLastTurn(prev)
            ? prev
            : updateLastTurn(prev, (tn) => ({
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

  const activeVoiceSession = voice && voice.phase !== 'idle' ? voice.sessionId : null;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      if (loginOpen) {
        event.stopPropagation();
        setLoginOpen(false);
      } else if (activeVoiceSession && isTauri) {
        // Esc during a recording only abandons that recording, never the window or task.
        event.stopPropagation();
        void lessComputerVoiceCancel(activeVoiceSession).catch(() => undefined);
      } else void windowAction('hide');
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [loginOpen, activeVoiceSession]);

  const working = turns.some((turn) => turn.status === 'working');
  const latestTurn = turns[turns.length - 1];
  const status = runStatus(latestTurn, t);
  const agentName = AGENTS.find((agent) => agent.id === provider)?.name ?? null;
  const voiceHint = voiceShortcut
    ? t(`lessComputer.voice.${voiceHintKey(voiceShortcut.mode)}`, { key: voiceShortcut.label })
    : null;
  const inspectorOpen = narrow ? inspectorOverlay : inspectorPreference;
  const toggleInspector = () => {
    if (narrow) {
      setInspectorOverlay((open) => !open);
      return;
    }
    const next = !inspectorPreference;
    setInspectorPreference(next);
    writeInspectorPreference(next);
  };
  const showPillStatus =
    status.tone === 'working' || status.tone === 'waiting' || status.tone === 'error';
  const openInspectorLabel = t('lessComputer.desktop.showInspector');

  return (
    <>
      <div
        ref={shellRef}
        className={`lc-desktop${inspectorOpen ? ' is-inspector-open' : ''}${closing ? ' is-closing' : ''}`}
      >
        <aside className="lc-sidebar" aria-label={t('lessComputer.desktop.agents')}>
          <div className="lc-sidebar-head" data-tauri-drag-region>
            <div className="lc-window-controls">
              <button
                className="lc-window-close"
                type="button"
                disabled={!isTauri}
                aria-label={t('lessComputer.closeTooltip')}
                onClick={() => void windowAction('hide')}
              >
                <XIcon />
              </button>
              <button
                className="lc-window-minimize"
                type="button"
                disabled={!isTauri}
                aria-label={t('lessComputer.desktop.minimize')}
                onClick={() => void windowAction('minimize')}
              >
                <MinusIcon />
              </button>
              <button
                className="lc-window-maximize"
                type="button"
                disabled={!isTauri}
                aria-label={t('lessComputer.desktop.maximize')}
                onClick={() => void windowAction('maximize')}
              >
                <Maximize2Icon />
              </button>
            </div>
            <Tooltip content={t('lessComputer.desktop.sessionUnavailable')} placement="bottom">
              <button
                className="lc-icon-button"
                type="button"
                aria-disabled="true"
                aria-label={t('lessComputer.desktop.sessionUnavailable')}
              >
                <SquarePenIcon />
              </button>
            </Tooltip>
          </div>
          <div className="lc-sidebar-scroll">
            <div className="lc-section-label">{t('lessComputer.desktop.agents')}</div>
            <ul className="lc-agent-list">
              {AGENTS.map((agent) => {
                const current = provider === agent.id;
                return (
                  <li
                    key={agent.id}
                    className={`lc-agent${current ? ' is-current' : ''}`}
                    aria-current={current ? 'true' : undefined}
                  >
                    <AgentAvatar agentId={agent.id} active={current} />
                    <div className="lc-agent-text">
                      <strong>{agent.name}</strong>
                      <span>
                        {current
                          ? sessionPreview(turns, status.label, t)
                          : t('lessComputer.desktop.agentSettings')}
                      </span>
                    </div>
                  </li>
                );
              })}
            </ul>
          </div>
          <div className="lc-sidebar-foot">
            <button
              className="lc-github"
              type="button"
              disabled={!isTauri || signedIn === true}
              onClick={() => setLoginOpen(true)}
            >
              <span className="lc-github-mark">
                <GithubMark />
              </span>
              <span className="lc-github-text">
                <strong>GitHub</strong>
                <small>
                  {signedIn === true
                    ? t('lessComputer.desktop.signedIn')
                    : t('lessComputer.desktop.signIn')}
                </small>
              </span>
              {signedIn === true ? <CheckIcon /> : <ChevronRightIcon />}
            </button>
          </div>
        </aside>

        <main className="lc-conversation" aria-label={t('lessComputer.desktop.currentSession')}>
          <header className="lc-chat-top" data-tauri-drag-region>
            <div className="lc-title-pill" data-tauri-drag-region>
              <AgentAvatar agentId={provider} size="sm" active={provider != null} />
              <strong data-tauri-drag-region>{agentName ?? t('lessComputer.title')}</strong>
              <span
                className={`lc-pill-status is-${status.tone}${showPillStatus ? ' is-visible' : ''}`}
                role="status"
              >
                <span aria-hidden="true" />
                {status.label}
              </span>
            </div>
            <div className="lc-chat-top-actions">
              {!inspectorOpen && (
                <Tooltip content={openInspectorLabel} placement="bottom">
                  <button
                    className="lc-icon-button"
                    type="button"
                    aria-label={openInspectorLabel}
                    onClick={toggleInspector}
                  >
                    <PanelRightOpenIcon />
                  </button>
                </Tooltip>
              )}
            </div>
          </header>
          {windowError && (
            <p className="lc-notice" role="alert">
              {t('lessComputer.desktop.windowError')}
            </p>
          )}
          <div className="lc-message-area">
            <MessageScrollerProvider
              key={sessionSeq}
              autoScroll
              defaultScrollPosition="last-anchor"
              scrollPreviousItemPeek={18}
            >
              {turns.length === 0 ? (
                <div className="lc-empty">
                  <AgentAvatar agentId={provider} size="lg" active={provider != null} />
                  <h2>{t('lessComputer.subtitle')}</h2>
                  <p>{t('lessComputer.desktop.emptyHint')}</p>
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
          <Composer
            working={working}
            voice={voice}
            agentName={agentName}
            voiceHint={voiceHint}
            focusAllowed={!login.mounted && !closing}
            t={t}
          />
        </main>

        <LessComputerInspector
          open={inspectorOpen}
          onClose={toggleInspector}
          t={t}
          summary={{
            agentId: provider,
            agentName,
            statusLabel: status.label,
            tone: status.tone,
            toolCount:
              latestTurn?.segments.filter((segment) => segment.kind === 'tool').length ?? 0,
            pendingApprovals:
              latestTurn?.status === 'working'
                ? latestTurn.segments.filter(
                    (segment) => segment.kind === 'approval' && !segment.decision,
                  ).length
                : 0,
            costUsd: latestTurn?.costUsd ?? null,
            voiceHint,
          }}
        />
      </div>
      {login.mounted && isTauri && (
        <GithubLoginModal
          closing={login.closing}
          overlayClassName="lc-dialog-overlay"
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

function runStatus(turn: Turn | undefined, t: Translate): { label: string; tone: RunTone } {
  if (!turn) return { label: t('lessComputer.desktop.idle'), tone: 'idle' };
  if (
    turn.status === 'working' &&
    turn.segments.some((segment) => segment.kind === 'approval' && !segment.decision)
  )
    return { label: t('lessComputer.desktop.waitingApproval'), tone: 'waiting' };
  if (turn.status === 'working') {
    const active = turn.segments.find((segment) => segment.kind === 'tool' && segment.running);
    return {
      label:
        active?.kind === 'tool'
          ? t(`lessComputer.activity.${toolActivityCategory(active.name)}Running`)
          : t('lessComputer.working'),
      tone: 'working',
    };
  }
  if (turn.status === 'done') return { label: t('lessComputer.done'), tone: 'done' };
  if (turn.status === 'cancelled') return { label: t('common.cancelled'), tone: 'cancelled' };
  if (turn.status === 'error') return { label: t('lessComputer.error'), tone: 'error' };
  return { label: t('lessComputer.desktop.idle'), tone: 'idle' };
}

function plainPreview(markdown: string): string {
  return markdown
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/[`*_>#~[\]()|]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Sidebar preview of the one real session: live status while working, else its latest words. */
function sessionPreview(turns: Turn[], statusLabel: string, t: Translate): string {
  const turn = turns[turns.length - 1];
  if (!turn) return t('lessComputer.desktop.idle');
  if (turn.status === 'working') return statusLabel;
  for (let i = turn.segments.length - 1; i >= 0; i -= 1) {
    const segment = turn.segments[i];
    if (segment.kind === 'text') {
      const preview = plainPreview(segment.content);
      if (preview) return preview;
    }
  }
  if (turn.status === 'error') return turn.errorMsg || statusLabel;
  return turn.user.trim() || statusLabel;
}

function voiceTime(elapsedMs: number): string {
  const seconds = Math.floor(Math.max(0, Number.isFinite(elapsedMs) ? elapsedMs : 0) / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  return typeof error === 'string' ? error : String(error);
}

function Composer({
  working,
  voice,
  t,
  agentName = null,
  voiceHint = null,
  focusAllowed = true,
}: {
  working: boolean;
  voice: LessComputerVoiceEvent | null;
  t: Translate;
  agentName?: string | null;
  voiceHint?: string | null;
  focusAllowed?: boolean;
}) {
  const [text, setText] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [failed, setFailed] = useState(false);
  const [voiceNotice, setVoiceNotice] = useState<string | null>(null);
  const [voicePending, setVoicePending] = useState(false);
  const [stoppingTask, setStoppingTask] = useState(false);
  const submittingRef = useRef(false);
  const composingRef = useRef(false);
  const voiceRequestRef = useRef(false);
  const focusPendingRef = useRef(false);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const phase = voice?.phase ?? 'idle';
  const speaking = voice !== null && phase !== 'idle';
  const dictating = speaking && voice?.mode === 'dictate';
  const busy = working || speaking || submitting || voicePending;
  const hasText = text.trim().length > 0;
  const voiceLabel =
    phase === 'recording'
      ? t('lessComputer.voice.listening')
      : phase === 'starting'
        ? t('lessComputer.voice.starting')
        : t('lessComputer.voice.transcribing');

  // A finished dictation lands in the draft exactly once, even when replayed.
  useEffect(() => {
    if (!voice || voice.phase !== 'idle' || voice.mode !== 'dictate' || !voice.outcome) return;
    if (!claimDictationResult(voice.sessionId)) return;
    const transcript = voice.transcript?.trim() ?? '';
    if (voice.outcome === 'committed' && transcript) {
      setText((current) => mergeDictation(current, transcript));
      setVoiceNotice(null);
      focusPendingRef.current = true;
    } else if (voice.outcome === 'empty') setVoiceNotice(t('lessComputer.voice.empty'));
    else if (voice.outcome === 'failed') setVoiceNotice(t('lessComputer.voice.failed'));
  }, [voice]);

  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.style.height = 'auto';
    input.style.height = `${Math.min(input.scrollHeight, MAX_INPUT_HEIGHT)}px`;
    if (!focusPendingRef.current) return;
    focusPendingRef.current = false;
    // A dialog or closing window owns focus. Consume this request rather than
    // unexpectedly replaying it when that surface is dismissed later.
    if (!focusAllowed) return;
    if (isTauri) void chatPanelFocusKeyboard().catch(() => undefined);
    input.focus({ preventScroll: true });
    input.setSelectionRange(input.value.length, input.value.length);
  }, [text, focusAllowed]);

  const send = async () => {
    const trimmed = text.trim();
    if (
      !isTauri ||
      !trimmed ||
      busy ||
      submittingRef.current ||
      voiceRequestRef.current ||
      composingRef.current
    )
      return;
    submittingRef.current = true;
    setSubmitting(true);
    setFailed(false);
    setVoiceNotice(null);
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
  const startVoice = async (mode: LessComputerVoiceMode) => {
    if (!isTauri || busy || voiceRequestRef.current || submittingRef.current) return;
    voiceRequestRef.current = true;
    setVoicePending(true);
    setVoiceNotice(null);
    setFailed(false);
    try {
      await lessComputerVoiceStart(mode);
    } catch (error) {
      setVoiceNotice(t('lessComputer.voice.startFailed', { message: errorText(error) }));
    } finally {
      voiceRequestRef.current = false;
      setVoicePending(false);
    }
  };
  const stopVoice = () => {
    if (!isTauri || !voice || !speaking || phase === 'transcribing') return;
    void lessComputerVoiceStop(voice.sessionId).catch(() =>
      setVoiceNotice(t('lessComputer.voice.failed')),
    );
  };
  const cancelVoice = () => {
    if (!isTauri || !voice || !speaking) return;
    void lessComputerVoiceCancel(voice.sessionId).catch(() => undefined);
  };
  const stopTask = async () => {
    if (!isTauri || !working || stoppingTask) return;
    setStoppingTask(true);
    setVoiceNotice(null);
    try {
      await lessComputerTaskCancel();
    } catch {
      setVoiceNotice(t('lessComputer.voice.taskCancelFailed'));
    } finally {
      setStoppingTask(false);
    }
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key !== 'Enter' || event.shiftKey) return;
    event.preventDefault();
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return;
    void send();
  };
  const liveTranscript = voice?.transcript ? transcriptTail(voice.transcript) : '';
  const placeholder = agentName
    ? t('lessComputer.desktop.messagePlaceholder', { agent: agentName })
    : t('lessComputer.inputPlaceholder');
  const primary = working ? 'stop' : hasText ? 'send' : 'voice';
  const primaryLabel =
    primary === 'stop'
      ? t('lessComputer.voice.stopTask')
      : primary === 'send'
        ? t('lessComputer.send')
        : t('lessComputer.voice.voiceMode');
  const confirmLabel = dictating
    ? t('lessComputer.voice.confirmDictation')
    : t('lessComputer.voice.stopAndSend');
  const caption = failed ? (
    <span role="alert">{t('lessComputer.desktop.sendError')}</span>
  ) : voiceNotice ? (
    <span role="alert">{voiceNotice}</span>
  ) : !isTauri ? (
    t('lessComputer.desktop.browserUnavailable')
  ) : speaking ? (
    dictating ? (
      t('lessComputer.voice.dictateCaption')
    ) : (
      t('lessComputer.voice.submitCaption')
    )
  ) : working ? (
    t('lessComputer.voice.stopHint')
  ) : (
    [t('lessComputer.desktop.inputHint'), voiceHint].filter(Boolean).join(' · ')
  );
  return (
    <div className="lc-composer-wrap">
      <form
        className={`lc-composer${speaking ? ' is-voice' : ''}${phase === 'recording' ? ' is-recording' : ''}`}
        onSubmit={(event) => {
          event.preventDefault();
          void send();
        }}
      >
        <textarea
          ref={inputRef}
          className="lc-text-input"
          rows={1}
          value={text}
          disabled={!isTauri || speaking}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={(event) => {
            setText(event.currentTarget.value);
            if (voiceNotice) setVoiceNotice(null);
          }}
          onKeyDown={onKeyDown}
          onCompositionStart={() => {
            composingRef.current = true;
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
          }}
          onFocus={() => {
            if (isTauri && focusAllowed) void chatPanelFocusKeyboard().catch(() => undefined);
          }}
          onPointerDown={() => {
            if (isTauri && focusAllowed) void chatPanelFocusKeyboard().catch(() => undefined);
          }}
        />
        <div className="lc-composer-actions">
          <Tooltip content={t('lessComputer.voice.dictate')} placement="top">
            <button
              className="lc-round lc-mic"
              type="button"
              disabled={!isTauri || busy || voicePending}
              aria-label={t('lessComputer.voice.dictate')}
              onClick={() => void startVoice('dictate')}
            >
              <MicIcon />
            </button>
          </Tooltip>
          <Tooltip content={primaryLabel} placement="top">
            <button
              className={`lc-round lc-primary lc-${primary}`}
              type={primary === 'send' ? 'submit' : 'button'}
              disabled={
                !isTauri ||
                (primary === 'stop'
                  ? stoppingTask
                  : primary === 'send'
                    ? busy
                    : busy || voicePending)
              }
              aria-label={primaryLabel}
              onClick={
                primary === 'stop'
                  ? () => void stopTask()
                  : primary === 'voice'
                    ? () => void startVoice('submit')
                    : undefined
              }
            >
              <span className="lc-icon-swap" key={primary}>
                {primary === 'stop' ? (
                  <SquareIcon />
                ) : primary === 'send' ? (
                  <ArrowUpIcon />
                ) : (
                  <AudioLinesIcon />
                )}
              </span>
            </button>
          </Tooltip>
        </div>
        <div className="lc-voice-stage" aria-hidden={!speaking}>
          {speaking && voice && (
            <>
              <Tooltip content={t('lessComputer.voice.cancel')} placement="top">
                <button
                  className="lc-round lc-ghost"
                  type="button"
                  aria-label={t('lessComputer.voice.cancel')}
                  onClick={cancelVoice}
                >
                  <XIcon />
                </button>
              </Tooltip>
              <div className="lc-voice-live">
                <LiveWaveform
                  level={phase === 'recording' ? voice.level : 0}
                  processing={phase !== 'recording'}
                  label={voiceLabel}
                />
                <p className={`lc-voice-transcript${liveTranscript ? '' : ' is-placeholder'}`}>
                  {liveTranscript || voiceLabel}
                </p>
              </div>
              <span className="lc-voice-clock">
                {phase === 'recording' && <span className="lc-recording-dot" aria-hidden="true" />}
                <time>{voiceTime(voice.elapsedMs)}</time>
              </span>
              <Tooltip content={confirmLabel} placement="top">
                <button
                  className="lc-round lc-primary"
                  type="button"
                  disabled={phase === 'transcribing'}
                  aria-label={confirmLabel}
                  onClick={stopVoice}
                >
                  {dictating ? <CheckIcon /> : <ArrowUpIcon />}
                </button>
              </Tooltip>
            </>
          )}
        </div>
      </form>
      <div className="lc-composer-caption" aria-live="polite">
        {caption}
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
          <div className="lc-user-row">
            <p className="lc-bubble lc-bubble-user">{turn.user}</p>
          </div>
        </MessageScrollerItem>
      )}
      <MessageScrollerItem messageId={`t${index}-assistant`} scrollAnchor={!hasUser}>
        <div className="lc-assistant-message">
          {turn.segments.map((segment, i) => {
            if (segment.kind === 'text') {
              const streaming = turn.status === 'working' && i === turn.segments.length - 1;
              return (
                <div className="lc-bubble-row" key={`s${i}`}>
                  <div className="lc-bubble lc-bubble-assistant lc-answer">
                    <AssistantMarkdown markdown={segment.content} streaming={streaming} />
                  </div>
                  {!streaming && segment.content.trim() && (
                    <CopyAction text={segment.content} t={t} />
                  )}
                </div>
              );
            }
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
            <div className="lc-typing" role="status" aria-label={t('lessComputer.working')}>
              <span />
              <span />
              <span />
            </div>
          )}
          {turn.status === 'error' && (
            <p className="lc-bubble lc-run-error" role="alert">
              <CircleAlertIcon />
              <span>{turn.errorMsg || t('lessComputer.error')}</span>
            </p>
          )}
          {turn.status === 'cancelled' && (
            <span className="lc-turn-footnote">
              <MinusIcon />
              {t('common.cancelled')}
            </span>
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
  const [open, setOpen] = useState(false);
  const groups = groupToolActivities(tools, working, interrupted);
  const active = groups.find((group) => group.state === 'active');
  const category =
    active?.category ??
    (groups.every((group) => group.category === groups[0]?.category)
      ? groups[0]?.category
      : undefined);
  const Icon = category ? CATEGORY_ICONS[category] : LayersIcon;
  return (
    <div className={`lc-tool-card${open ? ' is-open' : ''}${active ? ' is-active' : ''}`}>
      <button
        type="button"
        className="lc-tool-summary"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="lc-tool-icon" aria-hidden="true">
          <Icon />
        </span>
        <span className="lc-tool-heading">
          <span className={`lc-process-label${active ? ' is-running' : ''}`}>
            {active
              ? t(`lessComputer.activity.${active.category}Running`)
              : t('lessComputer.activity.process')}
          </span>
          <span className="lc-process-meta">
            <span className="lc-process-count">
              {t('lessComputer.activity.count', { count: tools.length })}
            </span>
            {!active && (
              <span className="lc-process-result">
                {interrupted
                  ? t('lessComputer.activity.stopped')
                  : t('lessComputer.activity.finished')}
              </span>
            )}
          </span>
        </span>
        <ChevronRightIcon className="lc-process-chevron" />
      </button>
      <div className="lc-tool-body" aria-hidden={!open}>
        <div className="lc-tool-body-inner">
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
        </div>
      </div>
    </div>
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
    <section
      className={`lc-approval${card.decision ? ' is-decided' : ''}${card.pending ? ' is-pending' : ''}`}
    >
      <div className="lc-approval-head">
        <span className="lc-approval-icon" aria-hidden="true">
          <ShieldCheckIcon />
        </span>
        <div className="lc-approval-heading">
          <strong>{t('lessComputer.approvalTitle')}</strong>
          {card.decision && <code className="lc-approval-command">{card.command}</code>}
        </div>
        {card.decision && (
          <span className={`lc-approval-result is-${card.decision}`}>
            <CheckIcon />
            {t(`lessComputer.desktop.${card.decision}Submitted`)}
          </span>
        )}
      </div>
      <div className="lc-approval-detail" aria-hidden={card.decision ? true : undefined}>
        <div className="lc-approval-detail-inner">
          <pre>{card.command}</pre>
          {card.reason && <p>{card.reason}</p>}
          <p className="lc-approval-warning">{t('lessComputer.approvalRerunWarning')}</p>
          {!card.decision && (
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
        </div>
      </div>
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
