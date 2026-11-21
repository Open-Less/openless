import { AskNotice } from './notices';
import { AiStatus, type AiStatusState } from '@/components/ui/ai-status';
import { ArrowDown, ArrowUpRight, BookOpen } from 'lucide-react';
import { AnimatePresence, motion, useReducedMotion } from 'motion/react';
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import {
  askLocale,
  type AskClient,
  type AskSource,
  type AskSubmission,
  type AskTurn,
  type AskTool,
  type SearchStatus,
} from './client';

const FADE =
  'linear-gradient(to bottom, transparent 0, rgba(0,0,0,0.04) 20px, rgba(0,0,0,0.15) 48px, rgba(0,0,0,0.35) 80px, rgba(0,0,0,0.6) 112px, rgba(0,0,0,0.85) 144px, black 176px, black calc(100% - 28px), transparent 100%)';

export const SPRING = { type: 'spring', duration: 0.5, bounce: 0.15 } as const;

const SEARCH_LABEL: Record<SearchStatus, string> = {
  idle: 'Not searched',
  disabled: 'Web Search is off',
  unsupported: 'Search is unsupported by this model',
  rejected: 'Search call was not accepted',
  snippet_only: 'Only unverified snippets',
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
  not_relevant: 'Not relevant',
  blocked: 'Page blocked',
  failed: 'Could not read',
  cancelled: 'Reading stopped',
};

function ToolActivity({ tools }: { tools?: AskTool[] }) {
  if (!tools?.length) return null;
  const chinese = /^zh\b/i.test(askLocale());
  const labels = chinese
    ? { running: '进行中', completed: '已完成', failed: '失败', cancelled: '已取消' }
    : { running: 'Running', completed: 'Completed', failed: 'Failed', cancelled: 'Cancelled' };
  const names: Record<string, string> = chinese
    ? { web_search: '联网搜索', calculator: '精确计算', read_context: '读取环境', read_page: '阅读网页', github_releases: '查询发布版本' }
    : { web_search: 'Web search', calculator: 'Calculate', read_context: 'Read context', read_page: 'Read page', github_releases: 'GitHub releases' };
  return <ul className="ask-tool-activity" aria-label={chinese ? '工具' : 'Tools'}>
    {tools.map((tool) => <li key={tool.toolCallId} data-tool-call-id={tool.toolCallId} data-status={tool.status}>
      <span>{names[tool.name] ?? tool.name}</span><span>{labels[tool.status]}</span>
    </li>)}
  </ul>;
}

function statusFor(turn: Pick<AskTurn, 'status' | 'searchStatus'> & { deepThinking?: boolean }): {
  state: AiStatusState;
  label?: string;
} {
  if (turn.status === 'failed') return { state: 'error' };
  if (turn.status === 'cancelled') return { state: 'error', label: 'Stopped · Incomplete' };
  if (turn.status === 'completed') return { state: 'done' };
  if (turn.searchStatus === 'searching')
    return { state: 'researching', label: SEARCH_LABEL.searching };
  if (turn.searchStatus === 'reading') return { state: 'analyzing', label: SEARCH_LABEL.reading };
  if (turn.status === 'writing') return { state: 'writing' };
  if (turn.status === 'thinking') {
    return { state: 'thinking', label: turn.deepThinking ? 'Thinking deeply' : 'Working' };
  }
  return { state: 'thinking', label: 'Working' };
}

export function ConversationLayout({
  expanded,
  turns,
  children,
  showSources,
  latestQuestion,
}: {
  expanded: boolean;
  turns: AskTurn[];
  children: ReactNode;
  showSources: (turnId: string) => void;
  latestQuestion?: string;
}) {
  const threadRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const follow = useRef(true);
  const [jumpVisible, setJumpVisible] = useState(false);
  const reduced = useReducedMotion();
  const refreshScroll = useCallback(() => {
    const thread = threadRef.current;
    if (!thread) return;
    if (follow.current) thread.scrollTop = thread.scrollHeight;
    setJumpVisible(!follow.current && thread.scrollHeight - thread.clientHeight - thread.scrollTop > 24);
  }, []);

  useLayoutEffect(() => {
    if (!expanded) {
      setJumpVisible(false);
      return;
    }
    follow.current = true;
    refreshScroll();
  }, [expanded, latestQuestion, refreshScroll]);

  const hasTurns = turns.length > 0;
  useEffect(() => {
    const thread = threadRef.current;
    const list = listRef.current;
    if (!expanded || !hasTurns || !thread || !list) return;
    const observer = new ResizeObserver(refreshScroll);
    observer.observe(thread);
    observer.observe(list);
    refreshScroll();
    return () => observer.disconnect();
  }, [expanded, hasTurns, refreshScroll]);
  const jumpLabel = /^zh-(TW|HK|Hant)\b/i.test(askLocale()) ? '跳到最新訊息'
    : /^zh\b/i.test(askLocale()) ? '跳到最新消息' : 'Jump to latest';

  return (
    <main className={expanded ? 'transcript-surface' : 'composer-surface'}>
      <div className="ask-layout-column">
        {expanded && turns.length > 0 && (
          <div
            ref={threadRef}
            className="ask-thread -mx-2 min-h-0 flex-1 overflow-y-auto px-2 pb-8 [scrollbar-width:none]"
            style={{ maskImage: FADE, WebkitMaskImage: FADE }}
            onScroll={(event) => {
              const thread = event.currentTarget;
              const atBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight <= 24;
              follow.current = atBottom;
              setJumpVisible(!atBottom);
            }}
          >
            <div ref={listRef} className="flex min-h-full flex-col justify-end gap-8 pt-40">
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
                      <div style={{ minHeight: 24 }}>
                        {turn.status !== 'failed' && <AiStatus state={state} label={label} />}
                      </div>
                      <ToolActivity tools={turn.tools} />
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

                      {turn.sources.length > 0 && (
                        <button
                          className="text-button turn-sources"
                          type="button"
                          onClick={() => showSources(turn.turnId)}
                        >
                          {turn.sources.some((source) => source.readStatus === 'read')
                            ? `${turn.sources.filter((source) => source.readStatus === 'read').length} sources`
                            : 'Sources'}
                        </button>
                      )}
                    </div>
                  </motion.article>
                );
              })}
            </div>
          </div>
        )}

        <div className="composer-content">
          {expanded && jumpVisible && (
            <button className="ask-jump-latest" type="button" onClick={(event) => {
              follow.current = true;
              event.currentTarget.parentElement
                ?.querySelector<HTMLTextAreaElement>('[data-slot="prompt-box-input"]')
                ?.focus({ preventScroll: true });
              refreshScroll();
            }}>
              <ArrowDown aria-hidden="true" size={14} />
              {jumpLabel}
            </button>
          )}
          {children}
        </div>
      </div>
    </main>
  );
}

export function SubmissionResult({
  submission,
  active,
}: {
  submission: AskSubmission;
  active: boolean;
}) {
  const text = submission.resultText;
  const { state, label } = statusFor(submission);
  const delivery =
    submission.delivery === 'written'
      ? submission.route === 'edit'
        ? 'Replaced in the original editor.'
        : 'Inserted in the original editor.'
      : submission.delivery === 'unknown'
        ? 'Check the original editor before pasting.'
        : submission.delivery === 'retained' && !submission.error
          ? 'Kept here because Ask could not write to the original editor.'
          : undefined;
  return (
    <section className="submission-result" aria-label="Ask result">
      {active && <AiStatus state={state} label={label} />}
      <AskNotice message={submission.copyStatus === 'failed' ? 'copy' : undefined} severity="warning" operation="ask-copy" />
      {delivery && <p role="status">{delivery}</p>}
      {submission.copyStatus === 'copied' && <p role="status">Copied to the clipboard.</p>}
      {!active && submission.status === 'cancelled' && <p role="status">Request stopped.</p>}
      {text && (submission.route === 'clarify' ? (
        <p className="result-text">{text}</p>
      ) : (
        <textarea key={submission.currentTurnId} className="result-text retained-result"
          aria-label="Retained result" defaultValue={text} rows={2} />
      ))}
    </section>
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
  client: Pick<AskClient, 'openSource'>;
  conversationId: string;
  turns: Pick<AskTurn, 'turnId' | 'searchStatus' | 'sources'>[];
  openedTurnId: string | undefined;
  show: (turnId: string) => void;
  close: () => void;
}) {
  const searched = turns.filter((turn) => turn.searchStatus !== 'idle');
  const open = searched.find((turn) => turn.turnId === openedTurnId);
  const turn = open ?? searched[searched.length - 1];
  const [failure, setFailure] = useState<{ turnId: string; message: unknown }>();
  if (!turn) return null;
  const readable = turn.sources.filter((source) => source.readStatus === 'read').length;
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
        Sources{readable ? ` · ${readable}` : ''}
      </button>
      {open && (
        <section id="ask-sources" aria-label="Web search sources" className="source-panel">
          <p role="status">{SEARCH_LABEL[open.searchStatus]}</p>
          <AskNotice
            severity="warning"
            operation="ask-source"
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
                          message: cause,
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
