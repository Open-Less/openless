// Fn+Space opens this conversation immediately. Closing dismisses the native
// session, so the next press starts a new one; an open panel keeps it and can
// take the next voice turn or a typed message.
import {
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpIcon,
  CheckIcon,
  CopyIcon,
  MicIcon,
  PencilLineIcon,
  QuoteIcon,
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
} from './chat/ui/message-scroller';
import { OpenLessPill } from '../capsule/OpenLessCapsule';
import { QaSurface } from './QaSurface';
import { VoiceWaveform } from './chat/VoiceWaveform';
import { AssistantMarkdown } from './chat/markdown';
import { useChatPanelLifecycle } from './chat/lifecycle';
import {
  chatPanelFocusKeyboard,
  listenQaDismissed,
  listenQaLevelChanges,
  listenQaStateChanges,
  qaSetEditInstructionMode,
  qaSubmitText,
  qaToggleRecording,
  qaWindowDismiss,
  qaCancelTurn,
  qaStopRecording,
  qaGetSnapshot,
} from '../../api/qa';
import {
  cancelSelectionPolishPreview,
  confirmSelectionPolishPreview,
  confirmSelectionVoicePreview,
  getSelectionPolishPreview,
  getSelectionVoicePreview,
  listenSelectionPolishPreviewHidden,
  listenSelectionPolishPreviewShown,
  revertSelectionVoicePreview,
} from '../../api/selection';
import { getRuntimeOS, isTauriHost } from '../../runtime/environment';
import { useModalActive } from '../../ui/layout/AppViewport';
import { useQaWindowLayout } from './useQaWindowLayout';
import { acceptQaSessionEvent, splitQaUserMessage } from '../../lib/qaMessage';
import { installMessageFollow } from '../../lib/messageFollow';
import type { QaChatMessage, QaStatePayload } from '../../api/contracts';
import { QaAnswerFooter } from './QaAnswerFooter';
import { qaSources } from './qaSources';
import { useQaPipeline } from './useQaPipeline';
import { stopDeepSeekReading } from './DeepSeekReadAloud';
import { copyTextToClipboard } from '../../api/files';
import './qa-panel.css';

type Status = 'idle' | 'recording' | 'thinking' | 'error';
type Translate = ReturnType<typeof useTranslation>['t'];
export interface QaPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
  onRequestDismiss?: () => Promise<boolean>;
}

export function QaPanel({ embedded = false, onRequestClose, onRequestDismiss }: QaPanelProps = {}) {
  const { t } = useTranslation();
  const modalActive = useModalActive();
  const pipelineMode = useQaPipeline();
  const isTauri = isTauriHost();
  const [messages, setMessages] = useState<QaChatMessage[]>([]);
  const [status, setStatus] = useState<Status>('idle');
  const [errorMsg, setErrorMsg] = useState('');
  const [selectionPreview, setSelectionPreview] = useState('');
  const [composerText, setComposerText] = useState('');
  const [streamingAnswer, setStreamingAnswer] = useState('');
  const [editApplyAvailable, setEditApplyAvailable] = useState(false);
  const [editRevertAvailable, setEditRevertAvailable] = useState(false);
  const [editApplyBusy, setEditApplyBusy] = useState(false);
  const [editInstructionMode, setEditInstructionMode] = useState(false);
  // The selection-polish preview shares this window; confirmation edits the original selection.
  const [polishResult, setPolishResult] = useState<{ text: string; sourceText: string } | null>(
    null,
  );
  const [polishError, setPolishError] = useState<string>('');
  const [polishBusy, setPolishBusy] = useState(false);
  const [level, setLevel] = useState(0);
  const [submitted, setSubmitted] = useState(false);
  const [submitBusy, setSubmitBusy] = useState(false);
  const [micBusy, setMicBusy] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  const [cancelBusy, setCancelBusy] = useState(false);
  const [activity, setActivity] = useState<QaStatePayload['activity']>();
  const [thoughtSeconds, setThoughtSeconds] = useState(0);
  const [thoughts, setThoughts] = useState<number[]>([]);
  const thought = useRef<{ sessionId: string | null; started: number } | null>(null);
  const activeSessionIdRef = useRef<string | null>(null);
  const polishRevision = useRef(0);
  const polishActiveRef = useRef(false);
  polishActiveRef.current = polishResult !== null;

  const loadPolishResult = async () => {
    const revision = ++polishRevision.current;
    try {
      const payload = await getSelectionPolishPreview();
      if (!payload || revision !== polishRevision.current) return;
      setPolishResult({ text: payload.text, sourceText: payload.sourceText });
      setPolishError('');
    } catch (error) {
      if (revision === polishRevision.current)
        console.error('[QaPanel] load polish preview failed', error);
    }
  };
  const loadPolishResultRef = useRef(loadPolishResult);
  loadPolishResultRef.current = loadPolishResult;
  const statusRef = useRef<Status>('idle');
  const panelEpoch = useRef(0);
  const nativeStateEpoch = useRef(0);
  const submissionRef = useRef<symbol | null>(null);
  const microphoneRef = useRef<symbol | null>(null);
  const editRequestRef = useRef<symbol | null>(null);
  const modeRequestRef = useRef<symbol | null>(null);
  const cancelRequestRef = useRef<symbol | null>(null);
  const dismissRef = useRef<symbol | null>(null);
  const nativeClosingRef = useRef(false);
  const [dismissEpoch, setDismissEpoch] = useState<number | null>(null);
  const tRef = useRef(t);
  tRef.current = t;
  const embeddedRef = useRef(embedded);
  embeddedRef.current = embedded;
  const onRequestCloseRef = useRef(onRequestClose);
  onRequestCloseRef.current = onRequestClose;
  const delegatedDismiss = useRef(onRequestDismiss);
  delegatedDismiss.current = onRequestDismiss;

  const changeStatus = (next: Status) => {
    statusRef.current = next;
    setStatus(next);
  };
  const invalidateRequests = () => {
    stopDeepSeekReading();
    panelEpoch.current += 1;
    polishRevision.current += 1;
    submissionRef.current = null;
    microphoneRef.current = null;
    editRequestRef.current = null;
    modeRequestRef.current = null;
    cancelRequestRef.current = null;
    setCancelBusy(false);
    thought.current = null;
    setThoughtSeconds(0);
    setActivity(undefined);
    setSubmitBusy(false);
    setMicBusy(false);
    setEditApplyBusy(false);
    setModeBusy(false);
  };

  const { enterEpoch, closing } = useChatPanelLifecycle(isTauri && !embedded, (next) => {
    nativeClosingRef.current = next;
    if (!next) return;
    // A fast reopen can arrive before React paints the exit. End the old session
    // at the close event so it cannot leak into the next presentation.
    invalidateRequests();
    activeSessionIdRef.current = null;
    changeStatus('idle');
    setMessages([]);
    setErrorMsg('');
    setStreamingAnswer('');
    setSelectionPreview('');
    setComposerText('');
    setEditInstructionMode(false);
    setPolishResult(null);
    setPolishError('');
    setPolishBusy(false);
    setEditApplyAvailable(false);
    setEditRevertAvailable(false);
    setLevel(0);
    setSubmitted(false);
    setActivity(undefined);
    setThoughts([]);
    setThoughtSeconds(0);
  });
  const presentationClosing = closing || dismissEpoch === enterEpoch;

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    const handles: (() => void)[] = [];
    const keep = (stop: () => void) => {
      if (cancelled) {
        stop();
        return false;
      }
      handles.push(stop);
      return true;
    };
    const unsubscribe = () => handles.splice(0).forEach((stop) => stop());
    void (async () => {
      try {
        let stateEpoch = 0;
        const applyState = (payload: QaStatePayload) => {
          if (cancelled || nativeClosingRef.current) return;
          const sessionEvent = acceptQaSessionEvent(activeSessionIdRef.current, payload);
          if (!sessionEvent.accepted) return;
          nativeStateEpoch.current += 1;
          if (activeSessionIdRef.current !== sessionEvent.sessionId) {
            stopDeepSeekReading();
            cancelRequestRef.current = null;
            setCancelBusy(false);
            thought.current = null;
            setThoughtSeconds(0);
            setActivity(undefined);
            setLevel(0);
            if (payload.kind === 'idle' || payload.kind === 'recording') {
              submissionRef.current = null;
              setSubmitBusy(false);
              if (!payload.messages?.length) setSubmitted(false);
            }
            // Apply/revert belongs to the captured QA session. A new turn must
            // not inherit a pending button or its late success/error response.
            editRequestRef.current = null;
            setEditApplyBusy(false);
            modeRequestRef.current = null;
            setModeBusy(false);
          }
          activeSessionIdRef.current = sessionEvent.sessionId;
          if (payload.messages) {
            setMessages(payload.messages);
            // An empty transcript is a new conversation. Follow-up turns keep the
            // history, so their thought durations stay aligned with each answer.
            if (payload.messages.length === 0) {
              setThoughts([]);
              setThoughtSeconds(0);
            }
          }
          if (typeof payload.editApplyAvailable === 'boolean')
            setEditApplyAvailable(payload.editApplyAvailable);
          if (typeof payload.editRevertAvailable === 'boolean')
            setEditRevertAvailable(payload.editRevertAvailable);
          if (typeof payload.editInstructionMode === 'boolean')
            setEditInstructionMode(payload.editInstructionMode);
          if (payload.kind !== 'recording') setLevel(0);
          if (payload.activity && (statusRef.current === 'thinking' || payload.kind === 'thinking'))
            setActivity(payload.activity);
          if (payload.kind === 'recording' || payload.kind === 'idle') setActivity(undefined);
          switch (payload.kind) {
            case 'activity':
              break;
            case 'idle':
              thought.current = null;
              changeStatus('idle');
              setSelectionPreview('');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              // Native ShowQa also sends a lightweight idle without messages;
              // it must not collapse an in-flight text submission.
              if (
                payload.messages?.length === 0 &&
                !submissionRef.current &&
                !microphoneRef.current
              )
                setSubmitted(false);
              break;
            case 'recording':
              thought.current = null;
              changeStatus('recording');
              setSelectionPreview(payload.selectionPreview ?? '');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'loading':
            case 'thinking':
              if (!thought.current) {
                thought.current = { sessionId: sessionEvent.sessionId, started: performance.now() };
                setThoughtSeconds(1);
              }
              changeStatus('thinking');
              setSubmitted(true);
              if (payload.selectionPreview != null) setSelectionPreview(payload.selectionPreview);
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'answer_delta':
              if (statusRef.current === 'thinking' && payload.chunk)
                setStreamingAnswer((previous) => previous + payload.chunk);
              break;
            case 'awaiting_approval':
              changeStatus('thinking');
              setSubmitted(true);
              break;
            case 'answer': {
              const timing = thought.current;
              thought.current = null;
              setActivity(undefined);
              const assistantCount =
                payload.messages?.filter((message) => message.role === 'assistant').length ?? 0;
              if (timing && timing.sessionId === sessionEvent.sessionId && assistantCount > 0) {
                const seconds = Math.max(
                  1,
                  Math.round((performance.now() - timing.started) / 1000),
                );
                setThoughts((previous) => {
                  const next = previous.slice();
                  next[assistantCount - 1] = seconds;
                  return next;
                });
              }
              changeStatus('idle');
              setErrorMsg('');
              setStreamingAnswer('');
              break;
            }
            case 'cancelled':
              invalidateRequests();
              changeStatus('idle');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'error':
              thought.current = null;
              setActivity(undefined);
              changeStatus('error');
              setErrorMsg(payload.error ?? tRef.current('qa.error'));
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
          }
        };
        const stateHandle = await listenQaStateChanges((payload) => {
          stateEpoch += 1;
          applyState(payload);
        });
        if (!keep(stateHandle)) return;
        const levelHandle = await listenQaLevelChanges((payload) => {
          if (
            cancelled ||
            statusRef.current !== 'recording' ||
            !payload.sessionId ||
            payload.sessionId !== activeSessionIdRef.current
          )
            return;
          setLevel(Number.isFinite(payload.level) ? Math.max(0, Math.min(1, payload.level)) : 0);
        });
        if (!keep(levelHandle)) return;
        const dismissHandle = await listenQaDismissed(() => {
          if (cancelled) return;
          invalidateRequests();
          activeSessionIdRef.current = null;
          setSelectionPreview('');
          setComposerText('');
          setLevel(0);
          if (embeddedRef.current && !delegatedDismiss.current) onRequestCloseRef.current?.();
        });
        if (!keep(dismissHandle)) return;
        // Native preview events are scoped to the desktop QA surface.
        if (!embeddedRef.current) {
          const shown = await listenSelectionPolishPreviewShown(() => {
            if (!cancelled) void loadPolishResultRef.current();
          });
          if (!keep(shown)) return;
          const hidden = await listenSelectionPolishPreviewHidden(() => {
            if (cancelled) return;
            polishRevision.current += 1;
            if (!polishActiveRef.current) return;
            setPolishResult(null);
            setPolishError('');
            void qaWindowDismiss().catch(() => undefined);
          });
          if (!keep(hidden)) return;
        }
        if (!embeddedRef.current) {
          void loadPolishResultRef.current();
        }
        // A lazily created desktop/embedded view can miss its initial recording
        // event. Subscribe first, then hydrate from the actual Core snapshot.
        // A state transition while the RPC is in flight supersedes that reply.
        for (
          let attempt = 0;
          attempt < 4 && !cancelled && !nativeClosingRef.current;
          attempt += 1
        ) {
          const observed = stateEpoch;
          const lifecycle = panelEpoch.current;
          let snapshot: QaStatePayload;
          try {
            snapshot = await qaGetSnapshot();
          } catch {
            // A failed hydration RPC must not tear down the live subscriptions.
            // Later native events remain authoritative, including a hotkey start.
            if (cancelled || panelEpoch.current !== lifecycle || stateEpoch !== observed) break;
            if (attempt === 3) {
              changeStatus('error');
              setErrorMsg(tRef.current('qa.error'));
            }
            continue;
          }
          if (cancelled || panelEpoch.current !== lifecycle) break;
          if (stateEpoch !== observed) continue;
          activeSessionIdRef.current = snapshot.sessionId ?? null;
          applyState(snapshot);
          break;
        }
      } catch (error) {
        unsubscribe();
        if (!cancelled) console.error('[QaPanel] listener setup failed', error);
      }
    })();
    return () => {
      cancelled = true;
      polishRevision.current += 1;
      stopDeepSeekReading();
      unsubscribe();
    };
  }, []);

  useEffect(() => {
    if (status !== 'thinking') return;
    const timer = setInterval(() => {
      if (thought.current)
        setThoughtSeconds(
          Math.max(1, Math.round((performance.now() - thought.current.started) / 1000)),
        );
    }, 200);
    return () => clearInterval(timer);
  }, [status]);

  const expanded =
    embedded ||
    polishResult !== null ||
    status !== 'idle' ||
    submitted ||
    messages.length > 0 ||
    editApplyAvailable ||
    editRevertAvailable;
  const activityLabel =
    activity?.kind === 'searching_web'
      ? t('qa.activity.searchingWeb')
      : activity?.kind === 'reading_web'
        ? t('qa.activity.readingWeb')
        : activity?.kind === 'using_tool'
          ? t('qa.activity.usingTool', { tool: activity.toolName || t('qa.activity.tool') })
          : t('capsule.thinking');
  const layout = useQaWindowLayout({
    expanded,
    enterEpoch,
    blocked: presentationClosing,
    nativeWindow: isTauri && !embedded,
  });

  const onClose = async () => {
    if (dismissRef.current) return;
    const request = Symbol('dismiss');
    dismissRef.current = request;
    setDismissEpoch(enterEpoch);
    invalidateRequests();
    const epoch = panelEpoch.current;
    try {
      let dismissed = true;
      if (embedded && onRequestDismiss) dismissed = await onRequestDismiss();
      else if (isTauri) await qaWindowDismiss();
      if (panelEpoch.current !== epoch) return;
      if (dismissed) onRequestCloseRef.current?.();
      else setDismissEpoch(null);
    } catch {
      if (panelEpoch.current === epoch) {
        changeStatus('error');
        setErrorMsg(t('qa.compact.closeError'));
        setDismissEpoch(null);
      }
    } finally {
      if (dismissRef.current === request) dismissRef.current = null;
    }
  };
  const onSubmitText = async () => {
    const text = composerText.trim();
    if (
      !isTauri ||
      !text ||
      statusRef.current === 'thinking' ||
      statusRef.current === 'recording' ||
      submissionRef.current ||
      microphoneRef.current ||
      editRequestRef.current ||
      modeRequestRef.current
    )
      return;
    const request = Symbol('submit');
    const epoch = panelEpoch.current;
    const originSession = activeSessionIdRef.current;
    submissionRef.current = request;
    setSubmitBusy(true);
    setSubmitted(true);
    setComposerText('');
    setErrorMsg('');
    try {
      await layout.waitForReady();
      if (
        panelEpoch.current !== epoch ||
        submissionRef.current !== request ||
        activeSessionIdRef.current !== originSession
      )
        return;
      await qaSubmitText(text, originSession);
    } catch {
      if (
        panelEpoch.current === epoch &&
        submissionRef.current === request &&
        activeSessionIdRef.current === originSession
      ) {
        setComposerText((current) => current || text);
        setErrorMsg(t('qa.compact.sendError'));
        changeStatus('error');
      }
    } finally {
      if (submissionRef.current === request) {
        submissionRef.current = null;
        setSubmitBusy(false);
      }
    }
  };
  const cancelTurnForSession = async (sessionId: string | null) => {
    if (
      !isTauri ||
      presentationClosing ||
      cancelRequestRef.current ||
      activeSessionIdRef.current !== sessionId ||
      (statusRef.current !== 'thinking' && statusRef.current !== 'recording')
    )
      return;
    const request = Symbol('cancel');
    cancelRequestRef.current = request;
    setCancelBusy(true);
    stopDeepSeekReading();
    const epoch = panelEpoch.current;
    const observedState = nativeStateEpoch.current;
    try {
      await qaCancelTurn();
    } catch {
      if (panelEpoch.current === epoch && nativeStateEpoch.current === observedState)
        setErrorMsg(t('common.operationFailed'));
    } finally {
      if (cancelRequestRef.current === request) {
        cancelRequestRef.current = null;
        setCancelBusy(false);
      }
    }
  };
  const currentSession = activeSessionIdRef.current;
  const onToggleRecording = async () => {
    if (
      !isTauri ||
      statusRef.current === 'thinking' ||
      microphoneRef.current ||
      submissionRef.current ||
      editRequestRef.current ||
      modeRequestRef.current
    )
      return;
    const request = Symbol('microphone');
    const epoch = panelEpoch.current;
    const observedState = nativeStateEpoch.current;
    microphoneRef.current = request;
    setMicBusy(true);
    setErrorMsg('');
    try {
      if (statusRef.current === 'recording') await qaStopRecording();
      else await qaToggleRecording();
    } catch {
      if (
        panelEpoch.current === epoch &&
        microphoneRef.current === request &&
        nativeStateEpoch.current === observedState
      ) {
        setErrorMsg(t('qa.compact.microphoneError'));
        changeStatus('error');
      }
    } finally {
      if (microphoneRef.current === request) {
        microphoneRef.current = null;
        setMicBusy(false);
      }
    }
  };
  const onEditInstructionModeChange = async (enabled: boolean) => {
    if (
      !isTauri ||
      statusRef.current === 'thinking' ||
      statusRef.current === 'recording' ||
      modeRequestRef.current ||
      submissionRef.current ||
      microphoneRef.current ||
      editRequestRef.current
    )
      return;
    const request = Symbol('mode');
    const epoch = panelEpoch.current;
    modeRequestRef.current = request;
    setModeBusy(true);
    try {
      await qaSetEditInstructionMode(enabled);
      if (panelEpoch.current === epoch && modeRequestRef.current === request)
        setEditInstructionMode(enabled);
    } catch {
      if (panelEpoch.current === epoch && modeRequestRef.current === request) {
        setErrorMsg(t('qa.compact.modeError'));
        changeStatus('error');
      }
    } finally {
      if (modeRequestRef.current === request) {
        modeRequestRef.current = null;
        setModeBusy(false);
      }
    }
  };
  // The action belongs to the preview actually rendered on screen. An event
  // may advance the ref before React commits the next frame.
  const displayedSessionId = activeSessionIdRef.current;
  const onEdit = async (revert: boolean) => {
    const session = displayedSessionId;
    if (
      !isTauri ||
      !session ||
      activeSessionIdRef.current !== session ||
      editRequestRef.current ||
      submissionRef.current ||
      microphoneRef.current ||
      modeRequestRef.current ||
      statusRef.current === 'thinking' ||
      statusRef.current === 'recording' ||
      (revert ? !editRevertAvailable : !editApplyAvailable)
    )
      return;
    const request = Symbol('edit');
    const epoch = panelEpoch.current;
    editRequestRef.current = request;
    setEditApplyBusy(true);
    setErrorMsg('');
    const current = () =>
      panelEpoch.current === epoch &&
      activeSessionIdRef.current === session &&
      editRequestRef.current === request;
    try {
      if (revert) await revertSelectionVoicePreview(session);
      else {
        const preview = await getSelectionVoicePreview(session);
        if (!current()) return;
        const text = preview?.text?.trim();
        if (!text) throw new Error('no_preview');
        await confirmSelectionVoicePreview(text, session);
      }
      if (current()) {
        if (!revert) setEditApplyAvailable(false);
        setEditRevertAvailable(false);
      }
    } catch {
      if (current()) {
        setErrorMsg(t('qa.editApplyUnavailable'));
        changeStatus('error');
      }
    } finally {
      if (editRequestRef.current === request) {
        editRequestRef.current = null;
        setEditApplyBusy(false);
      }
    }
  };

  const closePolishResult = () => {
    setPolishResult(null);
    setPolishError('');
    if (embeddedRef.current) onRequestCloseRef.current?.();
    else void qaWindowDismiss().catch(() => undefined);
  };

  const onPolishCancel = () => {
    if (polishBusy) return;
    const revision = ++polishRevision.current;
    setPolishBusy(true);
    void cancelSelectionPolishPreview()
      .then(() => {
        if (revision === polishRevision.current) closePolishResult();
      })
      .catch((error) => {
        if (revision === polishRevision.current) setPolishError(String(error));
      })
      .finally(() => {
        if (revision === polishRevision.current) setPolishBusy(false);
      });
  };

  const onPolishConfirm = () => {
    const text = polishResult?.text ?? '';
    if (polishBusy || !text.trim()) return;
    const revision = ++polishRevision.current;
    setPolishBusy(true);
    setPolishError('');
    void confirmSelectionPolishPreview(text)
      .then(() => {
        if (revision === polishRevision.current) closePolishResult();
      })
      .catch((error) => {
        if (revision === polishRevision.current) setPolishError(String(error));
      })
      .finally(() => {
        if (revision === polishRevision.current) setPolishBusy(false);
      });
  };

  const escape = useRef<() => void>();
  escape.current = () => {
    if (polishActiveRef.current) onPolishCancel();
    else if (statusRef.current === 'thinking' || statusRef.current === 'recording')
      void cancelTurnForSession(activeSessionIdRef.current);
    else void onClose();
  };
  useEffect(() => {
    if (modalActive || (embedded && onRequestDismiss)) return;
    const onKey = (event: KeyboardEvent) => {
      if (
        event.key !== 'Escape' ||
        event.isComposing ||
        event.keyCode === 229 ||
        event.defaultPrevented
      )
        return;
      event.preventDefault();
      escape.current?.();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [embedded, modalActive, onRequestDismiss]);

  const lastRole = messages[messages.length - 1]?.role;
  const questionLanded = lastRole === 'user' || streamingAnswer.length > 0;
  const firstQuestion = messages.find((message) => message.role === 'user');
  const heading = firstQuestion
    ? truncate(splitQaUserMessage(firstQuestion).question, 42)
    : t('qa.title');
  const thoughtByIndex = new Map<number, number>();
  let assistantCount = 0;
  messages.forEach((message, index) => {
    if (message.role !== 'assistant') return;
    const seconds = thoughts[assistantCount];
    if (seconds) thoughtByIndex.set(index, seconds);
    assistantCount += 1;
  });
  const liveActivity =
    status === 'thinking' && activity && !streamingAnswer ? activityLabel : undefined;
  const voiceActive =
    status === 'recording' ||
    (status === 'thinking' && !questionLanded && !submitBusy) ||
    (micBusy && status !== 'thinking');
  const inputBusy =
    status === 'thinking' ||
    status === 'recording' ||
    submitBusy ||
    micBusy ||
    modeBusy ||
    editApplyBusy;

  if (layout.failed && !presentationClosing) {
    return (
      <QaSurface
        closing={false}
        ready
        expanded={false}
        className="qa-capsule-shell"
        key={enterEpoch}
      >
        <div className="qa-layout-failure" role="alert">
          <span>{t('qa.compact.layoutError')}</span>
          <button
            type="button"
            className="qa-close"
            aria-label={t('common.close')}
            onClick={onClose}
          >
            <XIcon />
          </button>
        </div>
      </QaSurface>
    );
  }

  if (polishResult) {
    return (
      <QaSurface
        closing={presentationClosing}
        ready={layout.ready}
        expanded
        className={`qa-capsule-shell is-expanded${embedded ? ' is-embedded' : ''}${closing ? ' is-closing' : ''}`}
        key={enterEpoch}
      >
        <section
          className="qa-answer-panel"
          data-platform={getRuntimeOS()}
          aria-label={t('selectionPolishPreview.title')}
        >
          <header className="qa-answer-header" data-tauri-drag-region={embedded ? undefined : true}>
            <span className="qa-symbol" aria-hidden="true">
              ✳
            </span>
            <h1 data-tauri-drag-region={embedded ? undefined : true}>
              {t('selectionPolishPreview.title')}
            </h1>
            <span>{t('selectionPolishPreview.subtitle')}</span>
            <div className="qa-window-actions">
              <button
                type="button"
                className="qa-close"
                disabled={polishBusy}
                onClick={onPolishCancel}
                title={t('selectionPolishPreview.cancel')}
                aria-label={t('selectionPolishPreview.cancel')}
              >
                <XIcon />
              </button>
            </div>
          </header>
          {polishError && (
            <p className="qa-error" role="alert">
              {t('selectionPolishPreview.applyError')}
              {polishError}
            </p>
          )}
          <div className="qa-answer-scroll">
            <div className="qa-polish-result">
              <textarea
                aria-label={t('selectionPolishPreview.resultLabel')}
                className="qa-polish-text"
                value={polishResult.text}
                readOnly={polishBusy}
                onChange={(event) =>
                  setPolishResult({ ...polishResult, text: event.currentTarget.value })
                }
              />
              {polishResult.sourceText && (
                <p className="qa-polish-source">
                  {t('selectionPolishPreview.sourcePrefix')}
                  {polishResult.sourceText}
                </p>
              )}
            </div>
          </div>
          <div className="qa-edit-actions">
            <button type="button" disabled={polishBusy} onClick={onPolishCancel}>
              {t('selectionPolishPreview.cancel')}
            </button>
            <button
              type="button"
              disabled={polishBusy || !polishResult.text.trim()}
              onClick={onPolishConfirm}
            >
              <CheckIcon />
              {t('selectionPolishPreview.confirmReplace')}
            </button>
          </div>
        </section>
      </QaSurface>
    );
  }

  return (
    <QaSurface
      closing={presentationClosing}
      ready={layout.ready}
      expanded={expanded}
      className={`qa-capsule-shell${expanded ? ' is-expanded' : ''}${embedded ? ' is-embedded' : ''}${closing ? ' is-closing' : ''}${!expanded ? ' is-capsule' : ''}`}
      key={enterEpoch}
    >
      {!expanded && (
        <OpenLessPill
          state="recording"
          warming
          level={level}
          onCancel={onClose}
          onConfirm={onToggleRecording}
          leaving={presentationClosing}
        />
      )}
      {expanded && (
        <section
          className="qa-answer-panel qa-conversation"
          data-platform={getRuntimeOS()}
          aria-label={t('capsule.asking')}
        >
          <header className="qa-answer-header" data-tauri-drag-region={embedded ? undefined : true}>
            <h1 data-tauri-drag-region={embedded ? undefined : true}>{heading}</h1>
            <div className="qa-window-actions">
              <button
                type="button"
                className="qa-close"
                aria-label={t('common.close')}
                onClick={onClose}
              >
                <XIcon />
              </button>
            </div>
          </header>
          {status === 'thinking' && errorMsg && (
            <p role="alert" className="qa-turn-error">
              {errorMsg}
            </p>
          )}
          <QaAnswerScroll>
            <MessageScrollerProvider
              autoScroll={false}
              defaultScrollPosition="end"
              scrollEdgeThreshold={120}
              scrollPreviousItemPeek={18}
            >
              <MessageScroller>
                <MessageScrollerViewport>
                  <MessageScrollerContent
                    className="qa-messages"
                    aria-busy={status === 'thinking' || undefined}
                  >
                    {messages.map((message, index) => (
                      <MessageRow
                        key={index}
                        index={index}
                        message={message}
                        thoughtSeconds={thoughtByIndex.get(index)}
                        t={t}
                        canReadAloud={pipelineMode === 'deepseek'}
                      />
                    ))}
                    {status === 'thinking' && (
                      <MessageScrollerItem messageId="streaming">
                        <div className="qa-assistant-message">
                          <ThoughtStatus
                            live
                            seconds={thoughtSeconds}
                            activityLabel={liveActivity}
                            t={t}
                          />
                          {streamingAnswer && (
                            <AssistantMarkdown markdown={streamingAnswer} streaming />
                          )}
                        </div>
                      </MessageScrollerItem>
                    )}
                    {status === 'error' && (
                      <MessageScrollerItem messageId="error">
                        <ErrorContent error={errorMsg} t={t} />
                      </MessageScrollerItem>
                    )}
                    {messages.length === 0 &&
                      !streamingAnswer &&
                      status !== 'error' &&
                      status !== 'thinking' && (
                        <div className="qa-empty-answer">
                          <p>{t('qa.emptyTitle')}</p>
                          {status === 'idle' && (
                            <span>{submitBusy ? t('qa.compact.sending') : t('qa.emptyDesc')}</span>
                          )}
                        </div>
                      )}
                  </MessageScrollerContent>
                </MessageScrollerViewport>
                <MessageScrollerButton className="qa-jump" aria-label={t('qa.jumpToLatest')}>
                  {t('qa.jumpToLatest')}
                </MessageScrollerButton>
              </MessageScroller>
            </MessageScrollerProvider>
          </QaAnswerScroll>
          {editApplyAvailable && status === 'idle' && (
            <div className="qa-edit-actions">
              {editRevertAvailable && (
                <button
                  type="button"
                  disabled={!isTauri || editApplyBusy}
                  onClick={() => void onEdit(true)}
                >
                  {t('qa.editRevertPrevious')}
                </button>
              )}
              <button
                type="button"
                className="qa-apply"
                disabled={!isTauri || editApplyBusy}
                onClick={() => void onEdit(false)}
              >
                <CheckIcon />
                {t('qa.editApplyReplace')}
              </button>
            </div>
          )}
          <div className="qa-composer-dock">
            <Composer
              value={composerText}
              status={status}
              level={level}
              voiceActive={voiceActive}
              selectionPreview={selectionPreview}
              busy={inputBusy}
              micBusy={micBusy}
              embedded={embedded}
              editInstructionMode={editInstructionMode}
              onEditInstructionModeChange={onEditInstructionModeChange}
              onChange={setComposerText}
              onSubmit={onSubmitText}
              onToggleRecording={onToggleRecording}
              onCancelTurn={() => void cancelTurnForSession(currentSession)}
              cancelBusy={cancelBusy}
              t={t}
            />
          </div>
        </section>
      )}
    </QaSurface>
  );
}

function Composer({
  value,
  status,
  level,
  voiceActive,
  selectionPreview,
  busy,
  micBusy,
  embedded,
  editInstructionMode,
  onEditInstructionModeChange,
  onChange,
  onSubmit,
  onToggleRecording,
  onCancelTurn,
  cancelBusy,
  t,
}: {
  value: string;
  status: Status;
  level: number;
  voiceActive: boolean;
  selectionPreview: string;
  busy: boolean;
  micBusy: boolean;
  embedded: boolean;
  editInstructionMode: boolean;
  onEditInstructionModeChange: (enabled: boolean) => void;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onToggleRecording: () => void;
  onCancelTurn: () => void;
  cancelBusy: boolean;
  t: Translate;
}) {
  const isTauri = isTauriHost();
  const composingRef = useRef(false);
  const recording = status === 'recording';
  const generating = status === 'thinking';
  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key !== 'Enter') return;
    event.preventDefault();
    if (composingRef.current || event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (!busy && isTauri) onSubmit();
  };
  const label = recording
    ? t('overview.inAppDictation.recording')
    : micBusy
      ? t('common.loading')
      : t('overview.inAppDictation.processing');
  return (
    <form
      className={`qa-prompt${voiceActive ? ' is-voice' : ''}`}
      data-tauri-drag-region={embedded ? undefined : true}
      onSubmit={(event) => {
        event.preventDefault();
        if (!composingRef.current && !busy && isTauri) onSubmit();
      }}
    >
      <div className="qa-input-stage">
        <input
          value={value}
          disabled={!isTauri || recording || micBusy}
          placeholder={t('qa.composerPlaceholder')}
          aria-label={t('qa.composerPlaceholder')}
          onChange={(event) => onChange(event.currentTarget.value)}
          onKeyDown={onKeyDown}
          onCompositionStart={() => {
            composingRef.current = true;
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
          }}
          onFocus={() => {
            if (isTauri && !embedded) void chatPanelFocusKeyboard().catch(() => undefined);
          }}
          onPointerDown={() => {
            if (isTauri && !embedded) void chatPanelFocusKeyboard().catch(() => undefined);
          }}
        />
        {!isTauri && <span className="qa-browser-note">{t('qa.compact.browserUnavailable')}</span>}
      </div>
      <div className="qa-prompt-tools">
        <div className="qa-voice-pod">
          <div className="qa-voice-feedback" aria-hidden={!voiceActive}>
            {voiceActive && (
              <>
                <VoiceWaveform
                  level={recording ? level : 0}
                  processing={!recording}
                  label={label}
                />
                {selectionPreview && (
                  <span className="qa-recording-selection" title={selectionPreview}>
                    <QuoteIcon />
                    {truncate(selectionPreview, 32)}
                  </span>
                )}
              </>
            )}
          </div>
          <button
            className={`qa-mic${recording ? ' is-recording' : ''}`}
            type="button"
            disabled={!isTauri || generating || micBusy || (busy && !recording)}
            onClick={onToggleRecording}
            aria-pressed={recording}
            aria-label={recording ? t('qa.micStop') : t('qa.micLabel')}
            title={recording ? t('qa.micStop') : t('qa.micLabel')}
          >
            {recording ? <SquareIcon /> : <MicIcon />}
          </button>
        </div>
        <button
          className={`qa-mode${editInstructionMode ? ' is-on' : ''}`}
          type="button"
          aria-pressed={editInstructionMode}
          disabled={!isTauri || busy}
          onClick={() => onEditInstructionModeChange(!editInstructionMode)}
          aria-label={t('qa.editInstructionMode')}
          title={t('qa.editInstructionMode')}
        >
          <PencilLineIcon />
        </button>
        {generating ? (
          <button
            className="qa-send is-stop"
            type="button"
            disabled={!isTauri || cancelBusy}
            aria-label={t('qa.stop')}
            title={t('qa.stop')}
            onClick={onCancelTurn}
          >
            <SquareIcon />
          </button>
        ) : (
          <button
            className="qa-send"
            type="submit"
            disabled={!isTauri || busy || !value.trim()}
            aria-label={t('qa.composerSend')}
            title={t('qa.composerSend')}
          >
            <ArrowUpIcon />
          </button>
        )}
      </div>
    </form>
  );
}

function ThoughtStatus({
  live,
  seconds,
  activityLabel,
  t,
}: {
  live: boolean;
  seconds: number;
  activityLabel?: string;
  t: Translate;
}) {
  const elapsed = live ? t('qa.thoughtLive', { seconds }) : t('qa.thoughtDone', { seconds });
  const label = live && activityLabel ? `${elapsed} · ${activityLabel}` : elapsed;
  return (
    <div className={`qa-thought${live ? ' is-live' : ''}`} role="status">
      {live && <span className="qa-thinking-dot" />}
      {label}
    </div>
  );
}

function MessageRow({
  index,
  message,
  thoughtSeconds,
  t,
  canReadAloud = false,
}: {
  canReadAloud?: boolean;
  index: number;
  message: QaChatMessage;
  thoughtSeconds?: number;
  t: Translate;
}) {
  const answer = qaSources(message.content);
  if (message.role === 'user') {
    const { selection, question } = splitQaUserMessage(message);
    return (
      <MessageScrollerItem messageId={`m${index}`} scrollAnchor>
        <div className="qa-user-turn">
          {selection && <blockquote title={selection}>{truncate(selection, 120)}</blockquote>}
          <p>{question}</p>
          <CopyMessage text={question} />
        </div>
      </MessageScrollerItem>
    );
  }
  return (
    <MessageScrollerItem messageId={`m${index}`}>
      <div className="qa-assistant-message" aria-label={t('qa.compact.answer')}>
        {thoughtSeconds ? <ThoughtStatus live={false} seconds={thoughtSeconds} t={t} /> : null}
        <AssistantMarkdown markdown={answer.body} />
        <QaAnswerFooter
          text={message.content}
          sources={answer.sources}
          speechText={answer.body}
          canReadAloud={canReadAloud}
        />
      </div>
    </MessageScrollerItem>
  );
}
function CopyMessage({ text }: { text: string }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <button
      className="qa-copy"
      type="button"
      aria-label={t('common.copy')}
      onClick={() =>
        void copyTextToClipboard(text)
          .then(() => setCopied(true))
          .catch(() => setCopied(false))
      }
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
    </button>
  );
}
function ErrorContent({ error, t }: { error: string; t: Translate }) {
  const marker = '---model_output---',
    endMarker = '---end_model_output---';
  const start = error.indexOf(marker),
    after = start >= 0 ? error.slice(start + marker.length) : '';
  const end = after.indexOf(endMarker),
    raw = (end >= 0 ? after.slice(0, end) : after).trim();
  return (
    <div className="qa-error" role="alert">
      <p>{start < 0 ? error : error.slice(0, start).trim() || t('qa.error')}</p>
      {raw && <pre>{raw}</pre>}
      <span>{t('qa.errorRetryHint')}</span>
    </div>
  );
}
function truncate(text: string, max: number) {
  return text.length <= max ? text : `${text.slice(0, max)}…`;
}

function QaAnswerScroll({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => (ref.current ? installMessageFollow(ref.current) : undefined), []);
  return (
    <div ref={ref} className="qa-answer-scroll">
      {children}
    </div>
  );
}
