import { ToastNotificationCenter } from '../src/ui/feedback/ToastNotificationCenter';
import { AppViewport } from '../src/ui/layout/AppViewport';
import { InteractionProvider } from '../src/ui/overlays/InteractionProvider';
import { useEffect, useRef, useState } from 'react';
import { errorMessage, type AskClient, type AskFocus, type AskSnapshot } from './client';
import { useComposer } from './composer';
import { ConversationLayout, Sources, SubmissionResult } from './conversation';
import { connectAskFeed } from './feed';
import { useAskNotice } from './notices';
import { watchTheme } from './theme';
import './window.css';

export type AskSurface = 'composer' | 'conversation';

type SessionProps = {
  client: AskClient;
  snapshot: AskSnapshot;
  connected: boolean;
  check: () => void;
  focus?: AskFocus;
  activation: number;
  canDeliverVoice: () => boolean;
};

function ComposerSession(props: SessionProps) {
  const box = useComposer(props);
  const { client, snapshot } = props;
  const submission = snapshot.submission;
  const current = submission?.currentTurnId === snapshot.activeTurnId;
  const chatRoute = submission?.route === 'answer' || submission?.route === 'selectionAnswer';
  const [sourcesTurn, setSourcesTurn] = useState<string>();
  const sourceTurns =
    submission && !chatRoute
      ? [
          {
            turnId: submission.currentTurnId,
            searchStatus: submission.searchStatus,
            sources: submission.sources,
          },
        ]
      : [];
  return (
    <>
      <main className="composer-surface">
        <div className="composer-content">
          {submission && !chatRoute && (
            <SubmissionResult submission={submission} active={current} />
          )}
          {box}
        </div>
      </main>
      <Sources
        client={client}
        conversationId={snapshot.conversationId}
        turns={sourceTurns}
        openedTurnId={sourcesTurn}
        show={setSourcesTurn}
        close={() => setSourcesTurn(undefined)}
      />
    </>
  );
}

function ConversationSession({ client, snapshot, connected, check }: SessionProps) {
  const { conversationId, turns, activeTurnId } = snapshot;
  const [sourcesTurn, setSourcesTurn] = useState<string>();
  const [cancelling, setCancelling] = useState<string>();
  const notice = useAskNotice();
  const active = turns.some((turn) => turn.turnId === activeTurnId) ? activeTurnId : null;
  return (
    <>
      <ConversationLayout turns={turns} showSources={setSourcesTurn}>
        {active && (
          <button
            type="button"
            className="text-button stop-answer"
            disabled={!connected || cancelling === active}
            onClick={() => {
              setCancelling(active);
              void client
                .cancel(conversationId, active)
                .catch((error) =>
                  notice(errorMessage(error, 'Could not stop the answer.'), 'error', 'ask-stop'),
                )
                .finally(() => {
                  setCancelling(undefined);
                  check();
                });
            }}
          >
            Stop answer
          </button>
        )}
      </ConversationLayout>
      <Sources
        client={client}
        conversationId={conversationId}
        turns={turns}
        openedTurnId={sourcesTurn}
        show={setSourcesTurn}
        close={() => setSourcesTurn(undefined)}
      />
    </>
  );
}

export function AskWindow({
  client,
  surface = 'composer',
}: {
  client: AskClient;
  surface?: AskSurface;
}) {
  return (
    <AppViewport scale={false} className="ask-viewport">
      <InteractionProvider notificationHost={ToastNotificationCenter}>
        <AskScreen client={client} surface={surface} />
      </InteractionProvider>
    </AppViewport>
  );
}

function AskScreen({ client, surface }: { client: AskClient; surface: AskSurface }) {
  const notice = useAskNotice();
  const feed = useRef<ReturnType<typeof connectAskFeed>>();
  const shown = useRef(true);
  const hiding = useRef(false);
  const reconnect = useRef<() => void>(() => {});
  const [snapshot, setSnapshot] = useState<AskSnapshot | null>();
  const [connected, setConnected] = useState(false);
  const [windowFocused, setWindowFocused] = useState(() => document.hasFocus());
  const [focus, setFocus] = useState<AskFocus>();
  const [activation, setActivation] = useState(0);
  const [streamError, setStreamError] = useState<string>();
  const [platform, setPlatform] = useState<string>();
  const onWindows = platform ? platform === 'windows' : /Windows/i.test(navigator.userAgent);

  function suspend() {
    shown.current = false;
    feed.current?.disconnect();
    feed.current = undefined;
    setConnected(false);
  }

  useEffect(() => {
    let destroyed = false;
    let connection = 0;
    let unlisten: (() => void) | undefined;
    const theme = watchTheme();
    const updateFocus = ({ focusEpoch, focusRequest }: Partial<AskFocus>) => {
      if (surface !== 'composer' || focusEpoch === undefined || focusRequest === undefined) return;
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
              if (current.themeMode) theme.setTheme(current.themeMode);
              if (current.platform) setPlatform(current.platform);
              if (current.notice) notice(current.notice.message, 'warning', 'ask-window-notice');
              updateFocus(current);
            }
            return current;
          },
        },
        (view) => {
          setSnapshot(view);
          setStreamError(undefined);
          setConnected(true);
        },
        (cause) => {
          setStreamError(errorMessage(cause, 'Could not connect to Ask.'));
          setConnected(false);
        },
      );
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
        if (event.type === 'hidden') suspend();
        if (event.type === 'activated' || event.type === 'sessionChanged') {
          updateFocus(event);
          if (event.type === 'activated') {
            const wasHidden = !shown.current;
            shown.current = true;
            hiding.current = false;
            if (surface === 'composer') setActivation((previous) => previous + 1);
            setWindowFocused(document.hasFocus());
            if (wasHidden || !feed.current) connect();
            else feed.current.check();
          } else if (shown.current) {
            connect();
          }
        }
        if (event.type === 'themeChanged') theme.setTheme(event.themeMode);
        if (event.type === 'notice') notice(event.error.message, 'warning', 'ask-window-notice');
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
            errorMessage(cause, 'Could not connect to the Ask window.'),
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
  }, [client, notice, surface]);

  const Session = surface === 'composer' ? ComposerSession : ConversationSession;
  return (
    <>
      <div className="window-bar" data-tauri-drag-region data-windows={onWindows}>
        <button
          type="button"
          id="close-window"
          aria-label={surface === 'composer' ? 'Close Ask Anything' : 'Close Ask Conversation'}
          onClick={() => {
            if (hiding.current) return;
            hiding.current = true;
            suspend();
            void client
              .hide()
              .catch((cause) => {
                shown.current = true;
                notice(errorMessage(cause, 'Could not hide Ask.'), 'error', 'ask-hide');
                reconnect.current();
              })
              .finally(() => {
                hiding.current = false;
              });
          }}
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
      {snapshot ? (
        <Session
          key={snapshot.conversationId}
          client={client}
          snapshot={snapshot}
          connected={connected}
          focus={focus}
          activation={activation}
          canDeliverVoice={() =>
            shown.current &&
            windowFocused &&
            document.hasFocus() &&
            document.visibilityState !== 'hidden'
          }
          check={() => feed.current?.check()}
        />
      ) : (
        <p className="connection-notice" role="status">
          {snapshot === null
            ? 'No active conversation.'
            : streamError
              ? 'Ask is unavailable.'
              : 'Connecting…'}
        </p>
      )}
      {streamError && (
        <button
          title={streamError}
          className="text-button ask-reconnect"
          type="button"
          onClick={() => reconnect.current()}
        >
          Reconnect
        </button>
      )}
    </>
  );
}
