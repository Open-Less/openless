import { AppViewport } from '../src/ui/layout/AppViewport';
import { InteractionProvider } from '../src/ui/overlays/InteractionProvider';
import { useEffect, useRef, useState } from 'react';
import { errorMessage, type AskClient, type AskSnapshot } from './client';
import { useComposer } from './composer';
import { ConversationLayout, Sources } from './conversation';
import { connectAskFeed } from './feed';
import { useAskNotice } from './notices';
import { watchTheme } from './theme';
import './window.css';

function Session({
  client,
  snapshot,
  connected,
  canDeliverVoice,
  check,
  focusRevision,
}: {
  client: AskClient;
  snapshot: AskSnapshot;
  connected: boolean;
  canDeliverVoice: () => boolean;
  check: () => void;
  focusRevision: number;
}) {
  const [sourcesTurn, setSourcesTurn] = useState<string>();
  const { box, started } = useComposer({
    client,
    snapshot,
    connected,
    canDeliverVoice,
    check,
    focusRevision,
  });
  return (
    <>
      <ConversationLayout started={started} turns={snapshot.turns} showSources={setSourcesTurn}>
        {box}
      </ConversationLayout>
      <Sources
        client={client}
        conversationId={snapshot.conversationId}
        turns={snapshot.turns}
        openedTurnId={sourcesTurn}
        show={setSourcesTurn}
        close={() => setSourcesTurn(undefined)}
      />
    </>
  );
}

export function AskWindow({ client }: { client: AskClient }) {
  return (
    <AppViewport scale={false} className="ask-viewport">
      <InteractionProvider>
        <AskScreen client={client} />
      </InteractionProvider>
    </AppViewport>
  );
}

function AskScreen({ client }: { client: AskClient }) {
  const notice = useAskNotice();
  const feed = useRef<ReturnType<typeof connectAskFeed>>();
  const shown = useRef(true);
  const hiding = useRef(false);
  const reconnect = useRef<() => void>(() => {});
  const [snapshot, setSnapshot] = useState<AskSnapshot | null>();
  const [connected, setConnected] = useState(false);
  const [focusRevision, setFocusRevision] = useState(0);
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
    let unlisten: (() => void) | undefined;
    const theme = watchTheme();
    const scopedClient: AskClient = {
      ...client,
      async current() {
        const current = await client.current();
        if (!destroyed) {
          if (current.themeMode) theme.setTheme(current.themeMode);
          if (current.platform) setPlatform(current.platform);
          if (current.notice) notice(current.notice.message, 'warning', 'ask-selection');
        }
        return current;
      },
    };
    function connect() {
      feed.current?.disconnect();
      setConnected(false);
      feed.current = connectAskFeed(
        scopedClient,
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
      if (!shown.current || !feed.current) {
        shown.current = true;
        setFocusRevision((revision) => revision + 1);
        connect();
      } else {
        feed.current.check();
      }
    };
    const preventFileNavigation = (event: DragEvent) => {
      if (event.dataTransfer?.types.includes('Files')) event.preventDefault();
    };
    function onVisibilityChange() {
      if (document.visibilityState === 'hidden') suspend();
      else if (document.hasFocus()) resume();
    }
    void client
      .onWindowEvent((event) => {
        if (destroyed) return;
        if (event.type === 'hidden') suspend();
        if (event.type === 'activated') {
          shown.current = true;
          hiding.current = false;
          setFocusRevision((revision) => revision + 1);
          connect();
        }
        if (event.type === 'themeChanged') theme.setTheme(event.themeMode);
        if (event.type === 'notice') notice(event.error.message, 'warning', 'ask-window-notice');
      })
      .then((stop) => {
        if (destroyed) stop();
        else unlisten = stop;
      })
      .catch((cause) => {
        if (!destroyed)
          notice(
            errorMessage(cause, 'Could not connect to the Ask window.'),
            'error',
            'ask-connection',
          );
      });
    connect();
    window.addEventListener('focus', resume);
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
      window.removeEventListener('dragover', preventFileNavigation);
      window.removeEventListener('drop', preventFileNavigation);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [client, notice]);

  useEffect(() => {
    if (!streamError) return;
    const notification = notice(streamError, 'error', 'ask-stream');
    return notification.dismiss;
  }, [streamError, notice]);
  return (
    <>
      <div className="window-bar" data-tauri-drag-region data-windows={onWindows}>
        <button
          type="button"
          id="close-window"
          aria-label="Close Ask Anything"
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
          canDeliverVoice={() =>
            shown.current && document.hasFocus() && document.visibilityState !== 'hidden'
          }
          check={() => feed.current?.check()}
          focusRevision={focusRevision}
        />
      ) : (
        <ConversationLayout started={false} turns={[]} showSources={() => {}}>
          <p className="connection-notice" role="status">
            {snapshot === null
              ? 'Reopen Ask to start a conversation.'
              : streamError
                ? 'Ask is unavailable.'
                : 'Connecting…'}
          </p>
        </ConversationLayout>
      )}
      {streamError && (
        <button
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
