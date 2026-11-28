import { AppViewport } from '../src/ui/layout/AppViewport';
import { InteractionProvider } from '../src/ui/overlays/InteractionProvider';
import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { PromptBox } from '@/components/ui/prompt-box';
import { askErrorLabel, askLocale, isAskReferenceUnavailable, type AskClient, type AskFocus, type AskSnapshot } from './client';
import { askReferenceLabels } from '../src/i18n/askTurnReference';
import { useComposer } from './composer';
import { ConversationLayout, Sources, SubmissionResult } from './conversation';
import { connectAskFeed } from './feed';
import { AskNoticePopup, AskNoticeProvider, useAskNotice, useClearAskNotice, useCurrentAskNotice } from './notices';
import { watchTheme } from './theme';
import './window.css';

type SessionProps = {
  client: AskClient;
  snapshot: AskSnapshot;
  connected: boolean;
  check: () => void;
  focus?: AskFocus;
  activation: number;
  canDeliverVoice: () => boolean;
  expanded: boolean;
  captureSubmitReceipt: () => (turnId: string) => void;
  referenceBusy: boolean;
  referenceText?: string;
  seedDraft?: string;
  setReference: (text: string, draft: string) => void;
  removeReference: () => void;
};

type Expansion = { width: number; height: number; bottom: number };

function ComposerSession(props: SessionProps) {
  const { client, snapshot } = props;
  const submission = snapshot.submission;
  const current = submission?.currentTurnId === snapshot.activeTurnId;
  const chatRoute = submission?.route === 'answer' || submission?.route === 'selectionAnswer';
  const [sourcesTurn, setSourcesTurn] = useState<string>();
  const sourceTurns = [
    ...(props.expanded ? snapshot.turns : []),
    ...(submission && !chatRoute
      ? [
          {
            turnId: submission.currentTurnId,
            searchStatus: submission.searchStatus,
            sources: submission.sources,
          },
        ]
      : []),
  ];
  const report = useAskNotice();
  const reported = useRef(new Set<string>());
  const latest = snapshot.turns.at(-1);
  const matchingTurn = !submission || latest?.turnId === submission.currentTurnId ? latest : undefined;
  const errorTurn = submission?.currentTurnId ?? matchingTurn?.turnId;
  const failure = submission?.error ?? matchingTurn?.error;
  const searchFailure = submission?.searchError ?? matchingTurn?.searchError;
  const tools = submission?.tools ?? matchingTurn?.tools;
  useEffect(() => {
    for (const [kind, error] of [['request', failure], ['search', searchFailure], ...(tools ?? []).map((tool) => [tool.toolCallId, tool.error] as const)] as const) {
      if (!error || error.code === 'cancelled') continue;
      const key = `${errorTurn}:${kind}:${error.code}:${error.reason ?? ''}`;
      if (reported.current.has(key)) continue;
      reported.current.add(key);
      report(error, 'error');
    }
  }, [errorTurn, failure, searchFailure, tools, report]);
  const result = submission && (!current || props.expanded) && (!chatRoute || submission.resultText) ? (
    <SubmissionResult submission={submission} active={current} />
  ) : null;
  const box = useComposer({ ...props, result });
  return (
    <>
      <ConversationLayout
        expanded={props.expanded}
        turns={snapshot.turns}
        showSources={setSourcesTurn}
        latestQuestion={snapshot.turns.at(-1)?.turnId}
      >
        {box}
      </ConversationLayout>
      {props.expanded && <Sources
        client={client}
        conversationId={snapshot.conversationId}
        turns={sourceTurns}
        openedTurnId={sourcesTurn}
        show={setSourcesTurn}
        close={() => setSourcesTurn(undefined)}
      />}
    </>
  );
}

export function AskWindow({ client }: { client: AskClient }) {
  return (
    <AppViewport scale={false} className="ask-viewport">
      <InteractionProvider>
        <AskNoticeProvider><AskScreen client={client} /></AskNoticeProvider>
      </InteractionProvider>
    </AppViewport>
  );
}

function AskScreen({ client }: { client: AskClient }) {
  const notice = useAskNotice();
  const clearNotice = useClearAskNotice();
  const currentNotice = useCurrentAskNotice();
  const feed = useRef<ReturnType<typeof connectAskFeed>>();
  const shown = useRef(true);
  const hiding = useRef(false);
  const reconnect = useRef<() => void>(() => {});
  const presentation = useRef<HTMLDivElement>(null);
  const visibilityGeneration = useRef(0);
  const expanding = useRef<string>();
  const sessionId = useRef<string | null>();
  const referencedConversation = useRef<string>();
  const discardedReference = useRef<string>();
  const referenceOperation = useRef(false);
  const [referenceBusy, setReferenceBusy] = useState(false);
  const [referenceRecovery, setReferenceRecovery] = useState(false);
  const [manualReference, setManualReference] = useState<{ conversationId: string; text: string; draft: string }>();
  const [receipt, setReceipt] = useState<{ conversationId: string; turnId: string }>();
  const [expansion, setExpansion] = useState<Expansion>();
  const [snapshot, setSnapshot] = useState<AskSnapshot | null>();
  const [connected, setConnected] = useState(false);
  const [windowFocused, setWindowFocused] = useState(() => document.hasFocus());
  const [focus, setFocus] = useState<AskFocus>();
  const [activation, setActivation] = useState(0);
  const [platform, setPlatform] = useState<string>();
  const onWindows = platform ? platform === 'windows' : /Windows/i.test(navigator.userAgent);

  const suspend = useCallback(() => {
    shown.current = false;
    feed.current?.disconnect();
    feed.current = undefined;
    if (referencedConversation.current || referenceOperation.current) setSnapshot(null);
    referencedConversation.current = undefined;
    setManualReference(undefined);
    setReferenceRecovery(false);
    setConnected(false);
  }, []);

  const clearPresentation = useCallback(() => {
    visibilityGeneration.current++;
    expanding.current = undefined;
    clearNotice();
    setReceipt(undefined);
    setExpansion(undefined);
  }, [clearNotice]);

  const dismiss = useCallback(() => {
    if (hiding.current || !shown.current) return;
    hiding.current = true;
    visibilityGeneration.current++;
    suspend();
    void client
      .hide()
      .then(clearPresentation)
      .catch((cause) => {
        shown.current = true;
        notice(cause, 'error', 'ask-hide');
        reconnect.current();
      })
      .finally(() => {
        hiding.current = false;
      });
  }, [clearPresentation, client, notice, suspend]);

  useEffect(() => {
    function onEscape(event: KeyboardEvent) {
      if (
        event.key !== 'Escape' ||
        event.defaultPrevented ||
        event.isComposing ||
        event.keyCode === 229 ||
        event.repeat
      )
        return;
      event.preventDefault();
      if (snapshot?.activeTurnId) {
        void client.cancel(snapshot.conversationId, snapshot.activeTurnId).catch((cause) => {
          if (shown.current) notice(cause, 'error', 'ask-stop');
        });
      }
      dismiss();
    }
    window.addEventListener('keydown', onEscape);
    return () => window.removeEventListener('keydown', onEscape);
  }, [client, dismiss, notice, snapshot?.conversationId, snapshot?.activeTurnId]);

  useEffect(() => {
    let destroyed = false;
    let connection = 0;
    let unlisten: (() => void) | undefined;
    const theme = watchTheme();
    const updateFocus = ({ focusEpoch, focusRequest }: Partial<AskFocus>) => {
      if (focusEpoch === undefined || focusRequest === undefined) return;
      setFocus((previous) =>
        previous &&
        (previous.focusEpoch > focusEpoch ||
          (previous.focusEpoch === focusEpoch && previous.focusRequest >= focusRequest))
          ? previous
          : { focusEpoch, focusRequest },
      );
    };
    function connect() {
      const attempt = ++connection;
      feed.current?.disconnect();
      setConnected(false);
      feed.current = connectAskFeed(
        {
          ...client,
          async current() {
            const current = await client.current();
            if (!destroyed && shown.current && attempt === connection) {
              sessionId.current = current.conversationId;
              if (current.themeMode) theme.setTheme(current.themeMode);
              if (current.platform) setPlatform(current.platform);
              if (current.notice) notice(current.notice, 'warning', 'ask-window-notice');
              updateFocus(current);
            }
            return current;
          },
        },
        (view) => {
          const lostReference = !!referencedConversation.current &&
            (!view || (view.conversationId === referencedConversation.current && !view.turnReference));
          if (lostReference) discardedReference.current = referencedConversation.current;
          const discarded = !!view && view.conversationId === discardedReference.current;
          sessionId.current = view?.conversationId ?? null;
          referencedConversation.current = !discarded && view?.turnReference ? view.conversationId : undefined;
          setManualReference((current) =>
            !discarded && view?.turnReference?.source === 'userProvided' && current?.conversationId === view.conversationId
              ? current : undefined,
          );
          if (lostReference) {
            clearPresentation();
            notice({ code: 'askReferenceUnavailable' }, 'warning');
            setReferenceRecovery(true);
          } else if (view && !discarded) {
            discardedReference.current = undefined;
            setReferenceRecovery(false);
          }
          setSnapshot(lostReference || discarded ? null : view);
          setConnected(true);
        },
        (cause) => {
          if (isAskReferenceUnavailable(cause)) {
            invalidateReference(sessionId.current);
            return;
          }
          if (referencedConversation.current) {
            referencedConversation.current = undefined;
            setManualReference(undefined);
            setSnapshot(null);
            setReferenceRecovery(true);
            // Fetch a fresh native snapshot before showing the reference again.
            feed.current?.disconnect();
            feed.current = undefined;
          }
          notice(cause, 'error', 'ask-connection');
          setConnected(false);
        },
      );
    }
    function invalidateReference(invalidatedId?: string | null) {
      if (!shown.current || !invalidatedId || sessionId.current !== invalidatedId) return;
      discardedReference.current = invalidatedId;
      sessionId.current = null;
      referencedConversation.current = undefined;
      clearPresentation();
      setManualReference(undefined);
      setSnapshot(null);
      setReferenceRecovery(true);
      notice({ code: 'askReferenceUnavailable' }, 'warning');
      connect();
    }
    reconnect.current = connect;
    const resume = () => {
      if (hiding.current || document.visibilityState === 'hidden') return;
      setWindowFocused(document.hasFocus());
      if (!shown.current || !feed.current) {
        shown.current = true;
        connect();
      } else {
        feed.current.check();
      }
    };
    const preventFileNavigation = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes('Files')) event.preventDefault();
    };
    const onBlur = () => setWindowFocused(false);
    function onVisibilityChange() {
      if (document.visibilityState === 'hidden') suspend();
      else resume();
    }
    void client
      .onWindowEvent((event) => {
        if (destroyed) return;
        if (event.type === 'hidden') {
          clearPresentation();
          suspend();
        }
        if (event.type === 'referenceInvalidated') invalidateReference(event.conversationId);
        if (event.type === 'activated' || event.type === 'sessionChanged') {
          updateFocus(event);
          if (event.type === 'sessionChanged' && event.conversationId !== sessionId.current) {
            if (!event.conversationId && referencedConversation.current) setReferenceRecovery(true);
            sessionId.current = event.conversationId;
            clearPresentation();
            referencedConversation.current = undefined;
            setManualReference((current) =>
              current?.conversationId === event.conversationId ? current : undefined,
            );
            setSnapshot(null);
          }
          if (event.type === 'activated') {
            const wasHidden = !shown.current;
            shown.current = true;
            hiding.current = false;
            setActivation((previous) => previous + 1);
            setWindowFocused(document.hasFocus());
            if (wasHidden || !feed.current) connect();
            else feed.current.check();
          } else if (shown.current) {
            connect();
          }
        }
        if (event.type === 'themeChanged') theme.setTheme(event.themeMode);
        if (event.type === 'notice' && shown.current) notice(event.error, 'warning', 'ask-window-notice');
      })
      .then((stop) => {
        if (destroyed) stop();
        else {
          unlisten = stop;
          if (!feed.current && shown.current) connect();
        }
      })
      .catch((cause) => {
        if (!destroyed) {
          notice(
            cause,
            'error',
            'ask-connection',
          );
          if (!feed.current && shown.current) connect();
        }
      });
    window.addEventListener('focus', resume);
    window.addEventListener('blur', onBlur);
    window.addEventListener('dragover', preventFileNavigation);
    window.addEventListener('drop', preventFileNavigation);
    document.addEventListener('visibilitychange', onVisibilityChange);
    const checkTail = window.setInterval(() => {
      if (document.visibilityState === 'visible') feed.current?.check();
    }, 3000);
    return () => {
      destroyed = true;
      feed.current?.disconnect();
      feed.current = undefined;
      unlisten?.();
      theme.stop();
      window.clearInterval(checkTail);
      window.removeEventListener('focus', resume);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('dragover', preventFileNavigation);
      window.removeEventListener('drop', preventFileNavigation);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [clearNotice, clearPresentation, client, notice, suspend]);

  async function changeReference(run: () => Promise<void>) {
    if (referenceOperation.current || !shown.current) return;
    referenceOperation.current = true;
    setReferenceBusy(true);
    try {
      await run();
    } catch (error) {
      if (shown.current) notice(error, 'warning');
    } finally {
      referenceOperation.current = false;
      setReferenceBusy(false);
    }
  }

  function setReference(conversationId: string, text: string, draft: string) {
    void changeReference(async () => {
      if (sessionId.current !== conversationId) return;
      const reply = await client.setTurnReference(conversationId, text);
      if (!shown.current || (sessionId.current !== conversationId && sessionId.current !== reply.conversationId)) return;
      if (sessionId.current !== reply.conversationId) {
        clearPresentation();
        sessionId.current = reply.conversationId;
        setSnapshot(null);
      }
      referencedConversation.current = reply.conversationId;
      setManualReference({ conversationId: reply.conversationId, text, draft });
      reconnect.current();
    });
  }

  function startFresh(conversationId?: string) {
    void changeReference(async () => {
      if (conversationId) {
        if (sessionId.current !== conversationId) return;
        await client.revokeTurnReference(conversationId);
      }
      if (!shown.current) return;
      const current = await client.current();
      if (!shown.current) return;
      if (current.conversationId && current.conversationId !== conversationId) {
        reconnect.current();
        return;
      }
      setReferenceRecovery(true);
      const { conversationId: freshId } = await client.newConversation();
      if (!shown.current) return;
      if (sessionId.current !== freshId) {
        clearPresentation();
        setSnapshot(null);
      }
      referencedConversation.current = undefined;
      setManualReference(undefined);
      reconnect.current();
    });
  }

  const route = snapshot?.submission?.route;
  const routedTurn = snapshot?.submission?.currentTurnId;
  const conversationId = snapshot?.conversationId;
  useEffect(() => {
    if (
      expansion ||
      !connected ||
      !shown.current ||
      !conversationId ||
      !receipt ||
      receipt.conversationId !== conversationId ||
      receipt.turnId !== routedTurn ||
      (route !== 'answer' && route !== 'selectionAnswer') ||
      expanding.current === routedTurn
    )
      return;
    const frame = presentation.current;
    const box = frame?.querySelector<HTMLElement>('[data-slot="prompt-box"]');
    if (!frame || !box) return;
    const start = box.getBoundingClientRect();
    const bounds = frame.getBoundingClientRect();
    const from = { width: start.width, height: start.height, bottom: bounds.bottom - start.bottom };
    const generation = visibilityGeneration.current;
    expanding.current = receipt.turnId;
    void (client.expandChat?.(conversationId, receipt.turnId) ?? Promise.resolve())
      .then(() => {
        if (shown.current && visibilityGeneration.current === generation) setExpansion(from);
      })
      .catch((cause) => {
        if (shown.current && visibilityGeneration.current === generation) {
          notice(cause, 'error', 'ask-expand');
        }
      });
  }, [client, connected, conversationId, expansion, notice, receipt, route, routedTurn]);

  function captureSubmitReceipt() {
    const generation = visibilityGeneration.current;
    return (turnId: string) => {
      if (shown.current && visibilityGeneration.current === generation && conversationId) {
        setReceipt({ conversationId, turnId });
      }
    };
  }

  useEffect(() => {
    if (!currentNotice || expansion || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const surface = presentation.current?.querySelector('[data-slot="prompt-box"]');
    const animation = surface?.animate(
      [0, -4, 4, -3, 3, 0].map((x) => ({ transform: `translateX(${x}px)` })),
      { duration: 320, easing: 'ease-in-out' },
    );
    return () => animation?.cancel();
  }, [currentNotice?.id, expansion]);

  const expansionStyle =
    expansion &&
    ({
      '--ask-start-width': `${expansion.width}px`,
      '--ask-start-height': `${expansion.height}px`,
      '--ask-start-bottom': `${Math.max(0, expansion.bottom)}px`,
    } as CSSProperties);
  const manual = manualReference?.conversationId === snapshot?.conversationId ? manualReference : undefined;
  return (
    <div
      className="ask-presentation"
      ref={presentation}
      data-expanded={Boolean(expansion)}
      style={expansionStyle}
    >
      <AskNoticePopup expanded={Boolean(expansion)} />
      {expansion && <div className="ask-expansion-backdrop" aria-hidden="true" />}
      {expansion && (
        <div className="window-bar" data-tauri-drag-region data-windows={onWindows}>
          <button
            type="button"
            id="close-window"
            aria-label="Close Ask Conversation"
            onClick={dismiss}
          >
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path
                d="m4 4 8 8m0-8-8 8"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>
      )}
      {snapshot ? (
        <ComposerSession
          key={snapshot.conversationId}
          client={client}
          snapshot={snapshot}
          connected={connected}
          focus={focus}
          activation={activation}
          expanded={Boolean(expansion)}
          captureSubmitReceipt={captureSubmitReceipt}
          referenceBusy={referenceBusy}
          referenceText={manual?.text}
          seedDraft={manual?.draft}
          setReference={(text, draft) => setReference(snapshot.conversationId, text, draft)}
          removeReference={() => startFresh(snapshot.conversationId)}
          canDeliverVoice={() =>
            shown.current &&
            windowFocused &&
            document.hasFocus() &&
            document.visibilityState !== 'hidden'
          }
          check={() => feed.current?.check()}
        />
      ) : (
        <div className="composer-surface">
          <div className="ask-layout-column"><div className="composer-content">
            {referenceRecovery && (
              <button type="button" className="ask-reference-chip" disabled={referenceBusy}
                onClick={() => startFresh(sessionId.current ?? undefined)}>
                {askReferenceLabels(askLocale()).fresh}
              </button>
            )}
            {referenceRecovery && currentNotice && <span role="alert" className="sr-only">{currentNotice.message}</span>}
            <PromptBox
              className="w-full"
              placeholder={currentNotice ? askErrorLabel(currentNotice.code) : snapshot === null ? 'No active conversation.' : 'Connecting…'}
              submitDisabled
              inputProps={{ readOnly: true }}
            />
          </div></div>
        </div>
      )}
    </div>
  );
}
