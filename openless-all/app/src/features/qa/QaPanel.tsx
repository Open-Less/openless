// The recording shortcut plus the Ask companion key opens this conversation.
// Closing dismisses the native session; an open panel retains it across turns.
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { motion, MotionConfig } from 'framer-motion';
import { useReducedMotion } from '../../lib/motion';
import { getSettings } from '../../api/preferences';
import { CheckIcon, CopyIcon, XIcon } from 'lucide-react';
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
import { QaComposer, type QaStatus } from './QaComposer';
import { QaVoiceStrip } from './QaVoiceStrip';
import { QaStreamingResponse } from './QaStreamingResponse';
import { useChatPanelLifecycle } from './chat/lifecycle';
import {
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
import { useQaPipeline } from './useQaPipeline';
import { stopDeepSeekReading } from './DeepSeekReadAloud';
import { copyTextToClipboard } from '../../api/files';
import './qa-panel.css';

type Translate = ReturnType<typeof useTranslation>['t'];
export interface QaPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
  onRequestDismiss?: () => Promise<boolean>;
}

export function QaPanel({ embedded = false, onRequestClose, onRequestDismiss }: QaPanelProps = {}) {
  const { t } = useTranslation();
  const modalActive = useModalActive();
  const reduced = useReducedMotion();
  const pipelineMode = useQaPipeline();
  const isTauri = isTauriHost();
  const [messages, setMessages] = useState<QaChatMessage[]>([]);
  const [status, setStatus] = useState<QaStatus>('idle');
  const [errorMsg, setErrorMsg] = useState('');
  const [selectionPreview, setSelectionPreview] = useState('');
  const [composerText, setComposerText] = useState('');
  const [panelOpen, setPanelOpen] = useState(false);
  const [sendMorph, setSendMorph] = useState(false);
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
  const statusRef = useRef<QaStatus>('idle');
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

  const changeStatus = (next: QaStatus) => {
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
    setSendMorph(false);
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
    setPanelOpen(false);
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
          if (payload.panelOpen != null) setPanelOpen(payload.panelOpen);
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
              setSendMorph(false);
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
          setPanelOpen(false);
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
    panelOpen ||
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

  const composerRegion = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!isTauri || embedded || !panelOpen || !layout.ready || presentationClosing) return;
    let current = true;
    void getSettings()
      .then((prefs) => {
        if (current && prefs.qaShortcutBehavior === 'draft' && statusRef.current === 'idle') {
          composerRegion.current?.querySelector('textarea')?.focus({ preventScroll: true });
        }
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, [isTauri, embedded, panelOpen, layout.ready, presentationClosing, enterEpoch]);

  const scaledStyle = layout.contentScale > 1 ? { zoom: layout.contentScale } : undefined;
  // Motion measures projected bounds in viewport pixels but writes transforms
  // in local CSS pixels. Do not apply the window's zoom to that delta twice.
  const composerPoint = useCallback(
    ({ x, y }: { x: number; y: number }) => ({
      x: x / layout.contentScale,
      y: y / layout.contentScale,
    }),
    [layout.contentScale],
  );

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
    const draft = composerText;
    const text = draft.trim();
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
    setSendMorph(!reduced);
    setSubmitted(true);
    setComposerText('');
    setErrorMsg('');
    try {
      await layout.waitForReady();
      if (!reduced) await new Promise((resolve) => setTimeout(resolve, 245));
      if (
        panelEpoch.current !== epoch ||
        submissionRef.current !== request ||
        activeSessionIdRef.current !== originSession
      )
        return;
      setSendMorph(false);
      await qaSubmitText(text, originSession);
    } catch {
      if (
        panelEpoch.current === epoch &&
        submissionRef.current === request &&
        activeSessionIdRef.current === originSession
      ) {
        setComposerText((current) => current || draft);
        setErrorMsg(t('qa.compact.sendError'));
        changeStatus('error');
      }
    } finally {
      if (submissionRef.current === request) {
        submissionRef.current = null;
        setSendMorph(false);
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

  const showWelcome =
    (sendMorph || !submitBusy) &&
    messages.length === 0 &&
    status !== 'thinking' &&
    status !== 'error';
  const lastRole = messages[messages.length - 1]?.role;
  const questionLanded = lastRole === 'user' || streamingAnswer.length > 0;
  const firstQuestion = messages.find((message) => message.role === 'user');
  const heading = firstQuestion
    ? truncate(splitQaUserMessage(firstQuestion).question, 42)
    : t('qa.title');
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

  let assistantCount = 0;
  const turns = messages.map((message, index) => {
    const id = `m${index}`;
    if (message.role === 'user') {
      return (
        <MessageScrollerItem key={id} messageId={id} scrollAnchor>
          <QaUserMessage message={message} />
        </MessageScrollerItem>
      );
    }
    const elapsed = thoughts[assistantCount++];
    return (
      <MessageScrollerItem key={id} messageId={id}>
        <QaStreamingResponse
          markdown={message.content}
          elapsedSeconds={elapsed}
          canReadAloud={pipelineMode === 'deepseek'}
        />
      </MessageScrollerItem>
    );
  });
  if (status === 'thinking') {
    // Use the final answer's slot so finishing the stream doesn't replace the response element.
    const id = `m${messages.length}`;
    turns.push(
      <MessageScrollerItem key={id} messageId={id}>
        <QaStreamingResponse
          markdown={streamingAnswer}
          streaming
          elapsedSeconds={thoughtSeconds}
          activityLabel={liveActivity}
        />
      </MessageScrollerItem>,
    );
  }

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
          style={scaledStyle}
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
          style={scaledStyle}
          data-welcome={showWelcome ? 'true' : undefined}
          data-send-morph={sendMorph ? 'true' : undefined}
          data-platform={getRuntimeOS()}
          aria-label={t('capsule.asking')}
        >
          <header className="qa-answer-header" data-tauri-drag-region={embedded ? undefined : true}>
            {!showWelcome && (
              <h1 data-tauri-drag-region={embedded ? undefined : true}>{heading}</h1>
            )}
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
                    {turns}
                    {status === 'error' && (
                      <MessageScrollerItem messageId="error">
                        <ErrorContent error={errorMsg} t={t} />
                      </MessageScrollerItem>
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
          {sendMorph && (
            <div className="qa-send-morph-strip" aria-hidden="true">
              <QaVoiceStrip level={0} recording={false} />
            </div>
          )}
          <div className="qa-composer-dock" ref={composerRegion}>
            {showWelcome && <h1 className="qa-welcome-title">{t('qa.emptyTitle')}</h1>}
            <MotionConfig transformPagePoint={composerPoint}>
              <motion.div
                className="qa-composer-position"
                layout={reduced ? false : 'position'}
                transition={{ type: 'spring', duration: 0.5, bounce: 0.15 }}
              >
                <QaComposer
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
                  available={isTauri}
                />
              </motion.div>
            </MotionConfig>
          </div>
        </section>
      )}
    </QaSurface>
  );
}

function QaUserMessage({ message }: { message: QaChatMessage }) {
  const { selection, question } = splitQaUserMessage(message);
  return (
    <div className="qa-user-turn">
      {selection && <blockquote title={selection}>{truncate(selection, 120)}</blockquote>}
      <p>{question}</p>
      <CopyMessage text={question} />
    </div>
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
