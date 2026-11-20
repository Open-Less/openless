import { AskNotice } from './notices';
import { AiStatus, type AiStatusState } from '@/components/ui/ai-status';
import { ArrowUpRight, BookOpen } from 'lucide-react';
import { AnimatePresence, motion, useReducedMotion } from 'motion/react';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import {
  errorMessage,
  type AskClient,
  type AskSource,
  type AskTurn,
  type SearchStatus,
} from './client';

const FADE =
  'linear-gradient(to bottom, transparent 0, rgba(0,0,0,0.04) 20px, rgba(0,0,0,0.15) 48px, rgba(0,0,0,0.35) 80px, rgba(0,0,0,0.6) 112px, rgba(0,0,0,0.85) 144px, black 176px, black calc(100% - 28px), transparent 100%)';

export const SPRING = { type: 'spring', duration: 0.5, bounce: 0.15 } as const;

const SEARCH_LABEL: Record<SearchStatus, string> = {
  idle: 'Not searched',
  searching: 'Websearch · Searching…',
  reading: 'Websearch · Reading sources…',
  done: 'Search complete',
  no_results: 'No results',
  error: 'Search failed',
  cancelled: 'Search stopped',
};

const SOURCE_READ_LABEL: Record<AskSource['readStatus'], string> = {
  not_attempted: 'Not opened',
  reading: 'Reading',
  read: 'Read',
  blocked: 'Page blocked',
  failed: 'Could not read',
  cancelled: 'Reading stopped',
};

function statusFor(turn: AskTurn): { state: AiStatusState; label?: string } {
  if (turn.status === 'failed') return { state: 'error', label: 'Failed · Incomplete' };
  if (turn.status === 'cancelled') return { state: 'error', label: 'Stopped · Incomplete' };
  if (turn.status === 'completed') return { state: 'done' };
  if (turn.searchStatus === 'searching')
    return { state: 'researching', label: SEARCH_LABEL.searching };
  if (turn.searchStatus === 'reading') return { state: 'analyzing', label: SEARCH_LABEL.reading };
  if (turn.status === 'compacting') return { state: 'analyzing', label: 'Compact conversation' };
  if (turn.status === 'writing') return { state: 'writing' };
  if (turn.status === 'thinking') {
    return { state: 'thinking', label: turn.deepThinking ? 'Thinking deeply' : 'Working' };
  }
  return { state: 'thinking', label: 'Working' };
}

export function ConversationLayout({
  started,
  turns,
  children,
  showSources,
}: {
  started: boolean;
  turns: AskTurn[];
  children: ReactNode;
  showSources: (turnId: string) => void;
}) {
  const threadRef = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const lastScroll = useRef(0);
  const reduced = useReducedMotion();

  useEffect(() => {
    const thread = threadRef.current;
    if (thread && follow.current) {
      thread.scrollTo({ top: thread.scrollHeight, behavior: reduced ? 'auto' : 'smooth' });
    }
  }, [turns, reduced]);

  const latest = turns.at(-1);
  return (
    <div className="relative flex h-full flex-col items-center px-6 pt-6 pb-8">
      <AskNotice
        key={latest?.turnId}
        severity={latest?.error ? 'error' : 'warning'}
        message={latest?.error?.message ?? latest?.searchError?.message}
      />
      <div className="flex min-h-0 w-[560px] max-w-full flex-1 flex-col">
        {!started ? (
          <div className="flex flex-1 flex-col items-center justify-end pb-6">
            <h1 className="font-runde text-2xl font-semibold tracking-tight">
              What should we think about?
            </h1>
          </div>
        ) : (
          <div
            ref={threadRef}
            className="-mx-2 min-h-0 flex-1 overflow-y-auto px-2 pb-8 [scrollbar-width:none]"
            style={{ maskImage: FADE, WebkitMaskImage: FADE }}
            onScroll={(event) => {
              const thread = event.currentTarget;
              if (thread.scrollHeight - thread.scrollTop - thread.clientHeight < 72)
                follow.current = true;
              else if (thread.scrollTop < lastScroll.current) follow.current = false;
              lastScroll.current = thread.scrollTop;
            }}
          >
            <div className="flex min-h-full flex-col justify-end gap-8 pt-40">
              {turns.map((turn) => {
                const { state, label } = statusFor(turn);
                return (
                  <motion.article
                    key={turn.turnId}
                    initial={reduced ? false : { opacity: 0, y: 12, filter: 'blur(4px)' }}
                    animate={{ opacity: 1, y: 0, filter: 'blur(0px)' }}
                    transition={SPRING}
                    className="flex flex-col gap-3"
                  >
                    <div
                      className="flex flex-col items-end gap-1.5"
                      data-message-id={turn.userMessage.messageId}
                    >
                      <div className="user-bubble max-w-[80%] rounded-[20px] px-4 py-2.5 font-runde text-[15px] leading-6 whitespace-pre-wrap break-words">
                        {turn.userMessage.text && <p>{turn.userMessage.text}</p>}
                        {turn.attachments.length > 0 && (
                          <ul className="sent-attachments" aria-label="Sent attachments">
                            {turn.attachments.map((attachment) => (
                              <li key={attachment.attachmentId}>{attachment.displayName}</li>
                            ))}
                          </ul>
                        )}
                      </div>
                    </div>
                    <div className="flex flex-col gap-1">
                      <AiStatus state={state} label={label} />
                      <AnimatePresence initial={false}>
                        {turn.assistantMessage?.text && (
                          <motion.p
                            key={turn.assistantMessage.messageId}
                            data-message-id={turn.assistantMessage.messageId}
                            initial={reduced ? false : { opacity: 0, height: 0 }}
                            animate={{ opacity: 1, height: 'auto' }}
                            transition={SPRING}
                            className="assistant-text font-runde text-[15px] leading-6 whitespace-pre-wrap break-words"
                          >
                            {turn.assistantMessage.text}
                          </motion.p>
                        )}
                      </AnimatePresence>

                      {turn.searchStatus !== 'idle' && (
                        <button
                          className="text-button turn-sources"
                          type="button"
                          onClick={() => showSources(turn.turnId)}
                        >
                          {turn.sources.length
                            ? `${turn.sources.length} sources`
                            : SEARCH_LABEL[turn.searchStatus]}
                        </button>
                      )}
                    </div>
                  </motion.article>
                );
              })}
            </div>
          </div>
        )}

        <motion.div layout={reduced ? false : 'position'} transition={SPRING}>
          {children}
        </motion.div>

        {!started && <div className="flex-1" />}
      </div>
    </div>
  );
}

export function Sources({
  client,
  conversationId,
  turns,
  openedTurnId,
  show,
  close,
}: {
  client: AskClient;
  conversationId: string;
  turns: AskTurn[];
  openedTurnId: string | undefined;
  show: (turnId: string) => void;
  close: () => void;
}) {
  const searched = turns.filter((turn) => turn.searchStatus !== 'idle');
  const open = searched.find((turn) => turn.turnId === openedTurnId);
  const turn = open ?? searched[searched.length - 1];
  const [failure, setFailure] = useState<{ turnId: string; message: string }>();
  if (!turn) return null;
  return (
    <div className="sources">
      <button
        className="sources-toggle"
        type="button"
        aria-expanded={Boolean(open)}
        aria-controls={open ? 'ask-sources' : undefined}
        onClick={() => {
          if (open) close();
          else show(turn.turnId);
        }}
      >
        <BookOpen size={15} aria-hidden="true" />
        Sources{turn.sources.length ? ` · ${turn.sources.length}` : ''}
      </button>
      {open && (
        <section id="ask-sources" aria-label="Web search sources" className="source-panel">
          <p role="status">{SEARCH_LABEL[open.searchStatus]}</p>
          <AskNotice
            severity="warning"
            message={failure?.turnId === open.turnId ? failure.message : undefined}
          />
          <ul>
            {open.sources.map((source) => (
              <li key={source.sourceId}>
                <button
                  type="button"
                  className="source-item"
                  onClick={() => {
                    setFailure(undefined);
                    void client
                      .openSource(conversationId, open.turnId, source.sourceId)
                      .catch((cause) => {
                        setFailure({
                          turnId: open.turnId,
                          message: errorMessage(cause, 'Could not open this source.'),
                        });
                      });
                  }}
                >
                  <strong>
                    {source.title || source.url} <ArrowUpRight size={13} aria-hidden="true" />
                  </strong>
                  <small>{source.url}</small>
                  {source.snippet && <span>{source.snippet}</span>}
                  <small>{SOURCE_READ_LABEL[source.readStatus]}</small>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
