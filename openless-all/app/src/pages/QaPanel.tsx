// Compact QA composer. Native desktop starts at 480×80 and expands downwards
// after submission; embedded hosts retain their own frame and close callback.
import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpIcon,
  CheckIcon,
  MicIcon,
  PencilLineIcon,
  PlusIcon,
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
} from '../components/chat/ui/message-scroller';
import { UserAvatar, useGithubLogin } from '../components/chat/avatars';
import { VoiceWaveform } from '../components/chat/VoiceWaveform';
import { AssistantMarkdown } from '../components/chat/markdown';
import { useChatPanelLifecycle } from '../components/chat/lifecycle';
import {
  chatPanelFocusKeyboard,
  confirmSelectionVoicePreview,
  getSelectionVoicePreview,
  isTauri,
  qaSetEditInstructionMode,
  qaSubmitText,
  qaToggleRecording,
  qaWindowDismiss,
  qaWindowSetExpanded,
  qaGetSnapshot,
  revertSelectionVoicePreview,
} from '../lib/ipc';
import { acceptQaSessionEvent, splitQaUserMessage } from '../lib/qaMessage';
import type { QaChatMessage, QaStatePayload } from '../lib/types';
import './qa-panel.css';

type Status = 'idle' | 'recording' | 'thinking' | 'error';
type Translate = ReturnType<typeof useTranslation>['t'];
interface QaPanelProps {
  embedded?: boolean;
  onRequestClose?: () => void;
}
interface QaLevelPayload {
  sessionId: string;
  level: number;
}

export function QaPanel({ embedded = false, onRequestClose }: QaPanelProps = {}) {
  const { t } = useTranslation();
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
  const [level, setLevel] = useState(0);
  const [submitted, setSubmitted] = useState(false);
  const [submitBusy, setSubmitBusy] = useState(false);
  const [micBusy, setMicBusy] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  const [layoutError, setLayoutError] = useState(false);
  const activeSessionIdRef = useRef<string | null>(null);
  const statusRef = useRef<Status>('idle');
  const panelEpoch = useRef(0);
  const nativeStateEpoch = useRef(0);
  const submissionRef = useRef<symbol | null>(null);
  const microphoneRef = useRef<symbol | null>(null);
  const editRequestRef = useRef<symbol | null>(null);
  const modeRequestRef = useRef<symbol | null>(null);
  const dismissRef = useRef<symbol | null>(null);
  const resizeQueue = useRef<Promise<void>>(Promise.resolve());
  const layoutEpoch = useRef(0);
  const requestedLayout = useRef<{ expanded: boolean; promise: Promise<void> } | null>(null);
  const { enterEpoch, closing } = useChatPanelLifecycle();
  const tRef = useRef(t);
  tRef.current = t;
  const embeddedRef = useRef(embedded);
  embeddedRef.current = embedded;
  const onRequestCloseRef = useRef(onRequestClose);
  onRequestCloseRef.current = onRequestClose;
  const githubLogin = useGithubLogin(messages.filter((message) => message.role === 'user').length);

  const changeStatus = (next: Status) => {
    statusRef.current = next;
    setStatus(next);
  };
  const invalidateRequests = () => {
    panelEpoch.current += 1;
    submissionRef.current = null;
    microphoneRef.current = null;
    editRequestRef.current = null;
    modeRequestRef.current = null;
    setSubmitBusy(false);
    setMicBusy(false);
    setEditApplyBusy(false);
    setModeBusy(false);
  };

  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;
    const handles: (() => void)[] = [];
    void (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        let stateEpoch = 0;
        const applyState = (payload: QaStatePayload) => {
          if (cancelled) return;
          const sessionEvent = acceptQaSessionEvent(activeSessionIdRef.current, payload);
          if (!sessionEvent.accepted) return;
          nativeStateEpoch.current += 1;
          if (activeSessionIdRef.current !== sessionEvent.sessionId) {
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
          if (payload.messages) setMessages(payload.messages);
          if (typeof payload.editApplyAvailable === 'boolean')
            setEditApplyAvailable(payload.editApplyAvailable);
          if (typeof payload.editRevertAvailable === 'boolean')
            setEditRevertAvailable(payload.editRevertAvailable);
          if (typeof payload.editInstructionMode === 'boolean')
            setEditInstructionMode(payload.editInstructionMode);
          if (payload.kind !== 'recording') setLevel(0);
          switch (payload.kind) {
            case 'idle':
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
              changeStatus('recording');
              setSelectionPreview(payload.selectionPreview ?? '');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'loading':
            case 'thinking':
              changeStatus('thinking');
              setSubmitted(true);
              if (payload.selectionPreview != null) setSelectionPreview(payload.selectionPreview);
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'answer_delta':
              if (payload.chunk) setStreamingAnswer((previous) => previous + payload.chunk);
              break;
            case 'awaiting_approval':
              changeStatus('thinking');
              setSubmitted(true);
              break;
            case 'answer':
              changeStatus('idle');
              setErrorMsg('');
              setStreamingAnswer('');
              break;
            case 'cancelled':
              invalidateRequests();
              changeStatus('idle');
              setErrorMsg('');
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
            case 'error':
              changeStatus('error');
              setErrorMsg(payload.error ?? tRef.current('qa.error'));
              setStreamingAnswer('');
              setEditApplyAvailable(false);
              setEditRevertAvailable(false);
              break;
          }
        };
        const stateHandle = await listen<QaStatePayload>('qa:state', (event) => {
          stateEpoch += 1;
          applyState(event.payload);
        });
        handles.push(stateHandle);
        if (cancelled) {
          stateHandle();
          return;
        }
        const levelHandle = await listen<QaLevelPayload>('qa:level', (event) => {
          const payload = event.payload;
          if (
            cancelled ||
            statusRef.current !== 'recording' ||
            !payload.sessionId ||
            payload.sessionId !== activeSessionIdRef.current
          )
            return;
          setLevel(Number.isFinite(payload.level) ? Math.max(0, Math.min(1, payload.level)) : 0);
        });
        handles.push(levelHandle);
        if (cancelled) {
          levelHandle();
          return;
        }
        const dismissHandle = await listen('qa:dismiss', () => {
          if (cancelled) return;
          invalidateRequests();
          activeSessionIdRef.current = null;
          setSelectionPreview('');
          setComposerText('');
          setLevel(0);
          if (embeddedRef.current) onRequestCloseRef.current?.();
          else void qaWindowDismiss().catch(() => undefined);
        });
        handles.push(dismissHandle);
        if (cancelled) {
          dismissHandle();
          return;
        }
        // A lazily created desktop/embedded view can miss its initial recording
        // event. Subscribe first, then hydrate from the actual Core snapshot.
        // A state transition while the RPC is in flight supersedes that reply.
        for (let attempt = 0; attempt < 4 && !cancelled; attempt += 1) {
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
      } catch {
        handles.forEach((handle) => handle());
      }
    })();
    return () => {
      cancelled = true;
      handles.forEach((handle) => handle());
    };
  }, []);

  useEffect(() => {
    if (!closing) return;
    invalidateRequests();
    activeSessionIdRef.current = null;
    changeStatus('idle');
    setMessages([]);
    setErrorMsg('');
    setStreamingAnswer('');
    setSelectionPreview('');
    setComposerText('');
    setEditInstructionMode(false);
    setEditApplyAvailable(false);
    setEditRevertAvailable(false);
    setLevel(0);
    setSubmitted(false);
  }, [closing]);

  const expanded =
    embedded ||
    submitted ||
    messages.length > 0 ||
    streamingAnswer.length > 0 ||
    status === 'error' ||
    editApplyAvailable ||
    editRevertAvailable;
  const requestSize = (next: boolean) => {
    if (!isTauri || embeddedRef.current) return Promise.resolve();
    if (requestedLayout.current?.expanded === next) return requestedLayout.current.promise;
    // Serialize native geometry writes so a late expand cannot win over a later
    // compact request. Callers separately discard old lifecycle responses.
    const promise = resizeQueue.current
      .catch(() => undefined)
      .then(() => qaWindowSetExpanded(next));
    const request = { expanded: next, promise };
    requestedLayout.current = request;
    resizeQueue.current = promise;
    void promise.catch(() => {
      if (requestedLayout.current === request) requestedLayout.current = null;
    });
    return promise;
  };
  useEffect(() => {
    const epoch = ++layoutEpoch.current;
    if (closing || embedded || !isTauri) return;
    setLayoutError(false);
    void requestSize(expanded).catch(() => {
      if (layoutEpoch.current === epoch) setLayoutError(true);
    });
    return () => {
      layoutEpoch.current += 1;
    };
  }, [expanded, embedded, enterEpoch, closing]);

  const onClose = async () => {
    if (dismissRef.current) return;
    const request = Symbol('dismiss');
    dismissRef.current = request;
    invalidateRequests();
    const epoch = panelEpoch.current;
    try {
      if (isTauri) await qaWindowDismiss();
      if (panelEpoch.current === epoch) onRequestCloseRef.current?.();
    } catch {
      if (panelEpoch.current === epoch) {
        changeStatus('error');
        setErrorMsg(t('qa.compact.closeError'));
      }
    } finally {
      if (dismissRef.current === request) dismissRef.current = null;
    }
  };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.isComposing || event.keyCode === 229) return;
      event.preventDefault();
      void onClose();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

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
      await requestSize(true);
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
      await qaToggleRecording();
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

  const lastRole = messages[messages.length - 1]?.role;
  const questionLanded = lastRole === 'user' || streamingAnswer.length > 0;
  const thinkingRow = status === 'thinking' && !streamingAnswer && lastRole === 'user';
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

  return (
    <div
      className={`qa-capsule-shell${expanded ? ' is-expanded' : ''}${embedded ? ' is-embedded' : ''}${closing ? ' is-closing' : ''}`}
      key={enterEpoch}
    >
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
        onClose={onClose}
        t={t}
      />
      {expanded && (
        <section className="qa-answer-panel" aria-label={t('qa.title')}>
          <header className="qa-answer-header" data-tauri-drag-region={embedded ? undefined : true}>
            <span className="qa-symbol" aria-hidden="true">
              ✳
            </span>
            <h1 data-tauri-drag-region={embedded ? undefined : true}>{t('qa.title')}</h1>
            <span>{status === 'thinking' ? t('qa.thinking') : t('qa.compact.conversation')}</span>
          </header>
          {layoutError && (
            <p className="qa-layout-error" role="alert">
              {t('qa.compact.layoutError')}
            </p>
          )}
          <div className="qa-answer-scroll">
            <MessageScrollerProvider
              autoScroll
              defaultScrollPosition="last-anchor"
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
                        githubLogin={githubLogin}
                        t={t}
                      />
                    ))}
                    {streamingAnswer && (
                      <MessageScrollerItem messageId="streaming">
                        <div className="qa-assistant-message">
                          <AssistantLabel />
                          <AssistantMarkdown markdown={streamingAnswer} streaming />
                        </div>
                      </MessageScrollerItem>
                    )}
                    {thinkingRow && (
                      <MessageScrollerItem messageId="thinking">
                        <div className="qa-thinking" role="status">
                          <span className="qa-thinking-dot" />
                          {t('qa.thinking')}
                        </div>
                      </MessageScrollerItem>
                    )}
                    {status === 'error' && (
                      <MessageScrollerItem messageId="error">
                        <ErrorContent error={errorMsg} t={t} />
                      </MessageScrollerItem>
                    )}
                    {messages.length === 0 && !streamingAnswer && status !== 'error' && (
                      <div className="qa-empty-answer">
                        {submitBusy
                          ? t('qa.compact.sending')
                          : status === 'thinking'
                            ? t('overview.inAppDictation.processing')
                            : t('qa.emptyDesc')}
                      </div>
                    )}
                  </MessageScrollerContent>
                </MessageScrollerViewport>
                <MessageScrollerButton className="qa-jump" aria-label={t('qa.jumpToLatest')} />
              </MessageScroller>
            </MessageScrollerProvider>
          </div>
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
        </section>
      )}
    </div>
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
  onClose,
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
  onClose: () => void;
  t: Translate;
}) {
  const composingRef = useRef(false);
  const recording = status === 'recording';
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
      <button
        className="qa-add"
        type="button"
        disabled
        aria-label={t('qa.compact.addUnavailable')}
        title={t('qa.compact.addUnavailable')}
      >
        <PlusIcon />
      </button>
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
      <div className="qa-voice-pod">
        <div className="qa-voice-feedback" aria-hidden={!voiceActive}>
          {voiceActive && (
            <>
              <VoiceWaveform level={recording ? level : 0} processing={!recording} label={label} />
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
          disabled={!isTauri || status === 'thinking' || micBusy || (busy && !recording)}
          onClick={onToggleRecording}
          aria-pressed={recording}
          aria-label={recording ? t('qa.micStop') : t('qa.micLabel')}
          title={recording ? t('qa.micStop') : t('qa.micLabel')}
        >
          {recording ? <SquareIcon /> : <MicIcon />}
        </button>
      </div>
      <button
        className="qa-send"
        type="submit"
        disabled={!isTauri || busy || !value.trim()}
        aria-label={t('qa.composerSend')}
        title={t('qa.composerSend')}
      >
        <ArrowUpIcon />
      </button>
      <button
        className="qa-close"
        type="button"
        disabled={!isTauri && !embedded}
        onClick={onClose}
        aria-label={t('qa.closeTooltip')}
        title={t('qa.closeTooltip')}
      >
        <XIcon />
      </button>
    </form>
  );
}

function AssistantLabel() {
  return (
    <div className="qa-assistant-label">
      <span className="qa-symbol" aria-hidden="true">
        ✳
      </span>
      OpenLess
    </div>
  );
}
function MessageRow({
  index,
  message,
  githubLogin,
  t,
}: {
  index: number;
  message: QaChatMessage;
  githubLogin: string;
  t: Translate;
}) {
  if (message.role === 'user') {
    const { selection, question } = splitQaUserMessage(message);
    return (
      <MessageScrollerItem messageId={`m${index}`} scrollAnchor>
        <div className="qa-user-message">
          <span className="qa-user-avatar">
            <UserAvatar login={githubLogin} />
          </span>
          <div>
            {selection && <blockquote title={selection}>{truncate(selection, 120)}</blockquote>}
            <p>{question}</p>
          </div>
        </div>
      </MessageScrollerItem>
    );
  }
  return (
    <MessageScrollerItem messageId={`m${index}`}>
      <div className="qa-assistant-message" aria-label={t('qa.compact.answer')}>
        <AssistantLabel />
        <AssistantMarkdown markdown={message.content} />
      </div>
    </MessageScrollerItem>
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
