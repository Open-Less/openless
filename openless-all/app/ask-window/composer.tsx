import { useAskNotice } from './notices';
import { PromptBox, type PromptEffort } from '@/components/ui/prompt-box';
import { Brain, Paperclip } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { AttachmentDeck } from './attachments';
import {
  errorMessage,
  newRequestId,
  requestFingerprint,
  type AskClient,
  type AskSnapshot,
  type AskVoiceEvent,
  type SubmitAsk,
} from './client';
import { readDroppedImage } from './drop-image';

const PLACEHOLDERS = [
  'Ask anything',
  'Plan a weekend in Kyoto',
  "Explain springs like I'm five",
  'Draft a launch tweet',
];

const EFFORTS: PromptEffort[] = [
  { id: 'quick', label: 'Quick' },
  { id: 'deep', label: 'Think deeply', color: '#F2552C' },
];

function ActionButton({
  label,
  pressed,
  disabled,
  onClick,
  children,
}: {
  label: string;
  pressed?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      disabled={disabled}
      className="icon-button flex size-8 items-center justify-center rounded-full transition-colors"
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      {children}
    </button>
  );
}

type Selection = {
  start: number;
  end: number;
  direction: HTMLTextAreaElement['selectionDirection'];
};
type Draft = { text: string; selection?: Selection };
type PendingAsk = Omit<SubmitAsk, 'clientRequestId'>;
type Recording = { id: string } & (
  { mode: 'insert'; anchor: Draft } | { mode: 'submit'; request: Omit<SubmitAsk, 'text'> }
);
type VoiceRetry = { request: SubmitAsk };

function selectionOf(editor: HTMLTextAreaElement | null): Selection | undefined {
  if (!editor) return;
  return {
    start: editor.selectionStart,
    end: editor.selectionEnd,
    direction: editor.selectionDirection,
  };
}

export function useComposer({
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
  const { conversationId, attachments } = snapshot;
  const notice = useAskNotice();
  const sendNotice = useRef<ReturnType<typeof notice>>();
  const editor = useRef<HTMLTextAreaElement>(null);
  const draftRef = useRef('');
  const lastClear = useRef<Draft>();
  const nextSelection = useRef<Selection>();
  const composing = useRef(false);
  const draftIds = useRef(new Map<string, string>());
  const pendingRef = useRef(false);
  const activeRecording = useRef<Recording>();
  const seenRecordings = useRef(new Set<string>());
  const receiveVoice = useRef<(event: AskVoiceEvent) => void>(() => {});
  const mountId = useRef(newRequestId());
  const editorRevision = useRef(0);
  const editorFingerprint = useRef('');

  const [draft, setDraft] = useState('');
  const [focusSeen, setFocusSeen] = useState(-1);
  const [deepThinking, setDeepThinking] = useState(false);
  const [pending, setPending] = useState(false);
  const [acknowledgedTurn, setAcknowledgedTurn] = useState<string>();
  const [voiceRetries, setVoiceRetries] = useState<VoiceRetry[]>([]);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set());
  const [cancelling, setCancelling] = useState<string>();
  const [isComposing, setIsComposing] = useState(false);

  const turnId =
    snapshot.activeTurnId ??
    (acknowledgedTurn && !snapshot.turns.some((turn) => turn.turnId === acknowledgedTurn)
      ? acknowledgedTurn
      : undefined);
  const attachmentsReady = attachments.every((item) => item.state === 'ready');
  const cannotSubmit = !connected || pending || adding || removing.size > 0 || !attachmentsReady;

  function replaceDraft(text: string, selection?: Selection) {
    if (selection && text === draftRef.current && editor.current) {
      editor.current.setSelectionRange(
        selection.start,
        selection.end,
        selection.direction ?? undefined,
      );
    } else {
      nextSelection.current = selection;
    }
    draftRef.current = text;
    setDraft(text);
  }

  function syncEditor(force = false) {
    const input = editor.current;
    if (!connected || !input || !client.syncEditor || !canDeliverVoice()) return;
    const editable = document.activeElement === input && !composing.current && !input.readOnly;
    const selectionStart = input.selectionStart;
    const selectionEnd = input.selectionEnd;
    const attachmentIds = attachments.map((attachment) => attachment.attachmentId);
    const options = { deepThinking };
    const key = JSON.stringify([
      editable,
      selectionStart,
      selectionEnd,
      draftRef.current,
      attachmentIds,
      options,
    ]);
    if (!force && key === editorFingerprint.current) return;
    editorFingerprint.current = key;
    void client
      .syncEditor({
        conversationId,
        mountId: mountId.current,
        editorId: mountId.current,
        revision: ++editorRevision.current,
        editable,
        selectionStart,
        selectionEnd,
        attachmentIds,
        options,
      })
      .catch(() => {});
  }

  useLayoutEffect(() => {
    const selection = nextSelection.current;
    if (selection && editor.current) {
      editor.current.setSelectionRange(
        selection.start,
        selection.end,
        selection.direction ?? undefined,
      );
      nextSelection.current = undefined;
    }
    if (connected && focusSeen !== focusRevision && editor.current) {
      editor.current.focus({ preventScroll: true });
      setFocusSeen(focusRevision);
      syncEditor(true);
    } else {
      syncEditor();
    }
  });

  function keepVoice(request: SubmitAsk, message: string, severity: 'warning' | 'error') {
    sendNotice.current = notice(message, severity, 'ask-send');
    setVoiceRetries((retries) => [
      ...retries.filter((item) => item.request.clientRequestId !== request.clientRequestId),
      { request },
    ]);
  }

  function discardVoice(request: SubmitAsk) {
    setVoiceRetries((retries) =>
      retries.filter((item) => item.request.clientRequestId !== request.clientRequestId),
    );
  }

  function launch(request: SubmitAsk, restore?: Draft, fromVoice = false) {
    const attachmentIds = new Set(
      attachments
        .filter(
          (attachment) => attachment.state === 'ready' && !removing.has(attachment.attachmentId),
        )
        .map((attachment) => attachment.attachmentId),
    );
    const blocked =
      !connected ||
      pendingRef.current ||
      !!turnId ||
      adding ||
      request.attachmentIds.some((attachmentId) => !attachmentIds.has(attachmentId));
    if (blocked) {
      if (request.voiceRecordingId) void client.acknowledgeVoice?.(request.voiceRecordingId);
      if (restore) replaceDraft(restore.text, restore.selection);
      if (fromVoice) {
        keepVoice(
          request,
          !connected || pendingRef.current || turnId
            ? 'Ask is busy or reconnecting.'
            : 'An attachment from this recording is no longer ready.',
          'warning',
        );
      }
      return;
    }

    pendingRef.current = true;
    sendNotice.current?.dismiss();
    sendNotice.current = undefined;
    setPending(true);
    void client
      .submit(request)
      .then(({ turnId: acceptedTurnId }) => {
        setAcknowledgedTurn(acceptedTurnId);
        if (fromVoice) {
          discardVoice(request);
        } else {
          draftIds.current.delete(requestFingerprint(request));
        }
      })
      .catch((cause) => {
        const message = errorMessage(cause, 'Ask could not send this message.');
        const recoverable =
          typeof cause === 'object' &&
          cause !== null &&
          'code' in cause &&
          ['busy', 'invalid_argument', 'cancelled'].includes(String(cause.code));
        const severity = recoverable ? 'warning' : 'error';
        if (restore) replaceDraft(restore.text, restore.selection);
        if (fromVoice) {
          keepVoice(request, message, severity);
        } else {
          sendNotice.current = notice(message, severity, 'ask-send');
        }
      })
      .finally(() => {
        if (request.voiceRecordingId) void client.acknowledgeVoice?.(request.voiceRecordingId);
        pendingRef.current = false;
        setPending(false);
        check();
      });
  }

  function submitDraft(text: string, effort: PromptEffort) {
    const restore = lastClear.current;
    lastClear.current = undefined;
    const request: PendingAsk = {
      conversationId,
      text,
      attachmentIds: attachments.map((attachment) => attachment.attachmentId),
      options: { deepThinking: deepThinking || effort.id === 'deep' },
    };
    const fingerprint = requestFingerprint(request);
    let clientRequestId = draftIds.current.get(fingerprint);
    if (!clientRequestId) {
      clientRequestId = newRequestId();
      draftIds.current.set(fingerprint, clientRequestId);
    }
    launch({ ...request, clientRequestId }, restore);
  }

  function attach(add: () => Promise<unknown>) {
    setAdding(true);
    void add()
      .catch((cause) =>
        notice(errorMessage(cause, 'Could not add an attachment.'), 'warning', 'ask-attachment'),
      )
      .finally(() => {
        setAdding(false);
        check();
      });
  }

  useLayoutEffect(() => {
    receiveVoice.current = (event) => {
      if (event.conversationId !== conversationId || !canDeliverVoice()) return;
      const input = editor.current;
      if (event.type === 'started') {
        if (
          seenRecordings.current.has(event.recordingId) ||
          (activeRecording.current?.id === event.recordingId &&
            activeRecording.current.mode === event.mode)
        )
          return;
        activeRecording.current = undefined;
        if (event.mode === 'insert') {
          if (!input || document.activeElement !== input || input.readOnly || composing.current)
            return;
          activeRecording.current = {
            id: event.recordingId,
            mode: 'insert',
            anchor: { text: draftRef.current, selection: selectionOf(input) },
          };
        } else {
          activeRecording.current = {
            id: event.recordingId,
            mode: 'submit',
            request: {
              conversationId,
              clientRequestId: event.recordingId,
              voiceRecordingId: event.recordingId,
              attachmentIds: attachments.map((attachment) => attachment.attachmentId),
              options: { deepThinking },
            },
          };
        }
        return;
      }
      const recording = activeRecording.current;
      if (!recording || recording.id !== event.recordingId || recording.mode !== event.mode) {
        if (event.type === 'transcribed') void client.acknowledgeVoice?.(event.recordingId);
        return;
      }
      activeRecording.current = undefined;
      if (event.type !== 'transcribed' || !event.text.trim()) return;
      seenRecordings.current.add(event.recordingId);
      if (recording.mode === 'insert') void client.acknowledgeVoice?.(event.recordingId);
      if (recording.mode === 'submit') {
        launch({ ...recording.request, text: event.text.trim() }, undefined, true);
      } else if (
        input &&
        document.activeElement === input &&
        !input.readOnly &&
        !composing.current
      ) {
        const { text: original, selection } = recording.anchor;
        if (!selection || original !== draftRef.current) return;
        const caret = selection.start + event.text.length;
        replaceDraft(
          original.slice(0, selection.start) + event.text + original.slice(selection.end),
          {
            start: caret,
            end: caret,
            direction: 'none',
          },
        );
      }
    };
  });

  useEffect(() => {
    let stopped = false;
    let stop: (() => void) | undefined;
    const clearRecording = () => {
      activeRecording.current = undefined;
    };
    window.addEventListener('blur', clearRecording);
    void client
      .onVoiceEvent((event) => {
        if (!stopped) receiveVoice.current(event);
      })
      .then((unsubscribe) => {
        if (stopped) unsubscribe();
        else stop = unsubscribe;
      })
      .catch((cause) => {
        if (!stopped)
          notice(
            errorMessage(cause, 'Could not connect to Ask voice input.'),
            'warning',
            'ask-voice',
          );
      });
    return () => {
      stopped = true;
      window.removeEventListener('blur', clearRecording);
      stop?.();
    };
  }, [client, notice]);

  useLayoutEffect(() => {
    if (!connected) activeRecording.current = undefined;
  }, [connected]);

  const box = (
    <div
      className="composer"
      onDragOver={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = connected && !adding && !pending ? 'copy' : 'none';
      }}
      onDrop={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        if (!connected || adding || pendingRef.current) return;
        const files = event.dataTransfer.files;
        if (files.length !== 1) {
          notice('Drop one PNG or JPEG image at a time.', 'warning', 'ask-attachment');
          return;
        }
        const file = files[0];
        attach(async () => {
          const { displayName, dataBase64 } = await readDroppedImage(file);
          await client.addDroppedImage(conversationId, displayName, dataBase64);
        });
      }}
    >
      <PromptBox
        className="w-full"
        placeholder={PLACEHOLDERS}
        status={turnId ? 'streaming' : 'idle'}
        efforts={EFFORTS}
        value={draft}
        inputRef={editor}
        submitDisabled={cannotSubmit || isComposing}
        canSubmitEmpty={attachments.length > 0 && attachmentsReady}
        inputProps={{
          'aria-describedby': attachments.length ? 'draft-references' : undefined,
          readOnly: pending,
          onFocus: () => syncEditor(true),
          onBlur: () => syncEditor(true),
          onSelect: () => syncEditor(),
          onCompositionStart: () => {
            composing.current = true;
            setIsComposing(true);
          },
          onCompositionEnd: () => {
            composing.current = false;
            setIsComposing(false);
          },
        }}
        onValueChange={(value) => {
          if (pendingRef.current) return;
          lastClear.current =
            value === ''
              ? { text: draftRef.current, selection: selectionOf(editor.current) }
              : undefined;
          const recording = activeRecording.current;
          if (recording?.mode === 'insert' && value !== draftRef.current)
            activeRecording.current = undefined;
          replaceDraft(value);
        }}
        onSubmit={submitDraft}
        onStop={() => {
          if (!connected || !turnId || cancelling === turnId) return;
          setCancelling(turnId);
          void client
            .cancel(conversationId, turnId)
            .catch((cause) => {
              notice(
                errorMessage(cause, 'Could not stop the current answer.'),
                'error',
                'ask-stop',
              );
            })
            .finally(() => {
              setCancelling(undefined);
              check();
            });
        }}
        beforeInput={
          <AttachmentDeck
            client={client}
            conversationId={conversationId}
            attachments={attachments}
            removing={removing}
            disabled={pending || !connected}
            remove={(attachmentId) => {
              setRemoving((current) => new Set(current).add(attachmentId));
              void client
                .removeAttachment(conversationId, attachmentId)
                .catch((cause) => {
                  notice(
                    errorMessage(cause, 'Could not remove this attachment.'),
                    'warning',
                    'ask-attachment',
                  );
                })
                .finally(() => {
                  check();
                  setRemoving(
                    (current) => new Set([...current].filter((id) => id !== attachmentId)),
                  );
                });
            }}
          />
        }
        actions={
          <>
            <ActionButton
              label="Attach a file"
              disabled={adding || pending || !connected}
              onClick={() => attach(() => client.addAttachment(conversationId))}
            >
              <Paperclip size={18} />
            </ActionButton>
            <ActionButton
              label="Think deeply"
              pressed={deepThinking}
              onClick={() => setDeepThinking(!deepThinking)}
            >
              <Brain size={18} />
            </ActionButton>
          </>
        }
      />
      {voiceRetries.length > 0 && (
        <div className="voice-retries">
          {voiceRetries.map(({ request }) => (
            <div key={request.clientRequestId} className="composer-notice">
              <p className="voice-excerpt">{request.text}</p>
              <button
                className="text-button"
                type="button"
                disabled={!connected || pending || Boolean(turnId)}
                onClick={() => launch({ ...request, voiceRecordingId: undefined }, undefined, true)}
              >
                Retry recording
              </button>
              <button
                className="text-button"
                type="button"
                disabled={pending}
                onClick={() => discardVoice(request)}
              >
                Discard
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
  return { box, started: pending || Boolean(acknowledgedTurn) || snapshot.turns.length > 0 };
}
