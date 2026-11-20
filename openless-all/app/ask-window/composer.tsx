import { useAskNotice } from './notices';
import { PromptBox, type PromptEffort } from '@/components/ui/prompt-box';
import { Brain, Paperclip } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { AttachmentDeck } from './attachments';
import {
  askLocale,
  errorMessage,
  newRequestId,
  requestFingerprint,
  submissionErrorMessage,
  type AskError,
  type AskClient,
  type AskFocus,
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
type Recording = { id: string; draft: string; input: HTMLTextAreaElement };

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
  focus,
  activation,
  expanded,
  captureSubmitReceipt,
}: {
  client: AskClient;
  snapshot: AskSnapshot;
  connected: boolean;
  canDeliverVoice: () => boolean;
  check: () => void;
  focus?: AskFocus;
  activation: number;
  expanded: boolean;
  captureSubmitReceipt: () => (turnId: string) => void;
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
  const receiveVoice = useRef<(event: AskVoiceEvent) => void>(() => {});
  const currentVoiceEditor = useRef<(event: AskVoiceEvent) => HTMLTextAreaElement | undefined>();
  const syncSelection = useRef(() => {});
  const mountId = useRef(newRequestId());
  const editorRevision = useRef(0);
  const editorFingerprint = useRef('');
  const focusSeen = useRef('');

  const [draft, setDraft] = useState('');
  const [deepThinking, setDeepThinking] = useState(false);
  const [pending, setPending] = useState(false);
  const [acknowledgedTurn, setAcknowledgedTurn] = useState<string>();
  const [voiceListening, setVoiceListening] = useState(false);
  const [voicePolishing, setVoicePolishing] = useState(false);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set());
  const [cancelling, setCancelling] = useState<string>();
  const [isComposing, setIsComposing] = useState(false);

  const turnId =
    snapshot.activeTurnId ??
    (acknowledgedTurn &&
    snapshot.submission?.currentTurnId !== acknowledgedTurn &&
    !snapshot.turns.some((turn) => turn.turnId === acknowledgedTurn)
      ? acknowledgedTurn
      : undefined);
  const attachmentsReady = attachments.every((item) => item.state === 'ready');
  const cannotSubmit = !connected || pending || adding || removing.size > 0 || !attachmentsReady;
  const submission = snapshot.submission;
  const showPolish =
    !expanded &&
    (voicePolishing ||
      (submission?.currentTurnId === turnId &&
        submission?.route === 'edit' &&
        submission.status === 'writing'));
  const language = askLocale();
  const polishLabel = /^zh-(TW|HK|Hant)\b/i.test(language)
    ? '正在潤飾…'
    : /^zh\b/i.test(language)
      ? '正在润色…'
      : 'Polishing…';

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

  function syncEditor(editorBlurred = false) {
    const input = editor.current;
    if (
      !connected ||
      !input ||
      !focus ||
      !voiceListening ||
      !client.syncEditor ||
      !canDeliverVoice()
    )
      return;
    const editable =
      !editorBlurred && document.activeElement === input && !composing.current && !input.readOnly;
    const selectionStart = input.selectionStart;
    const selectionEnd = input.selectionEnd;
    const key = JSON.stringify([
      focus.focusEpoch,
      editable,
      selectionStart,
      selectionEnd,
      draftRef.current,
    ]);
    const changed = key !== editorFingerprint.current;
    const requestKey = `${focus.focusEpoch}:${focus.focusRequest}:${activation}`;
    if (!changed && requestKey === focusSeen.current) return;
    if (changed) {
      editorFingerprint.current = key;
      editorRevision.current += 1;
      activeRecording.current = undefined;
      setVoicePolishing(false);
    }
    focusSeen.current = requestKey;
    void client
      .syncEditor(focus, {
        conversationId,
        mountId: mountId.current,
        editorId: mountId.current,
        revision: editorRevision.current,
        editable,
        selectionStart,
        selectionEnd,
      })
      .then((accepted) => {
        if (!accepted && focusSeen.current === requestKey) focusSeen.current = '';
      })
      .catch(() => {
        if (focusSeen.current === requestKey) focusSeen.current = '';
      });
  }

  const focused = useRef('');
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
    const token = focus ? `${focus.focusEpoch}:${focus.focusRequest}` : '';
    if (connected && token !== focused.current && editor.current && canDeliverVoice()) {
      focused.current = token;
      editor.current.focus({ preventScroll: true });
    }
    syncSelection.current = () => syncEditor();
    syncEditor();
  });

  useEffect(() => {
    const input = editor.current;
    const onSelectionChanged = () => syncSelection.current();
    input?.addEventListener('select', onSelectionChanged);
    document.addEventListener('selectionchange', onSelectionChanged);
    return () => {
      input?.removeEventListener('select', onSelectionChanged);
      document.removeEventListener('selectionchange', onSelectionChanged);
    };
  }, []);

  function launch(request: SubmitAsk, restore?: Draft) {
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
      if (restore) replaceDraft(restore.text, restore.selection);
      return;
    }

    pendingRef.current = true;
    const reportAccepted = captureSubmitReceipt();
    sendNotice.current?.dismiss();
    sendNotice.current = undefined;
    setPending(true);
    void client
      .submit(request)
      .then(({ turnId: acceptedTurnId }) => {
        setAcknowledgedTurn(acceptedTurnId);
        reportAccepted(acceptedTurnId);
        draftIds.current.delete(requestFingerprint(request));
      })
      .catch((cause) => {
        const message =
          typeof cause === 'object' && cause !== null && 'reason' in cause
            ? (submissionErrorMessage(cause as AskError) ?? 'Ask could not send this message.')
            : errorMessage(cause, 'Ask could not send this message.');
        const recoverable =
          typeof cause === 'object' &&
          cause !== null &&
          'code' in cause &&
          ['busy', 'invalid_argument', 'cancelled'].includes(String(cause.code));
        const severity = recoverable ? 'warning' : 'error';
        if (restore) replaceDraft(restore.text, restore.selection);
        sendNotice.current = notice(message, severity, 'ask-send');
      })
      .finally(() => {
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
    currentVoiceEditor.current = (event) => {
      const input = editor.current;
      if (
        connected &&
        canDeliverVoice() &&
        event.conversationId === conversationId &&
        event.focusEpoch === focus?.focusEpoch &&
        event.mountId === mountId.current &&
        event.editorId === mountId.current &&
        event.revision === editorRevision.current &&
        input &&
        document.activeElement === input &&
        !input.readOnly &&
        !composing.current &&
        input.value === draftRef.current &&
        event.selectionStart === input.selectionStart &&
        event.selectionEnd === input.selectionEnd
      )
        return input;
    };
    receiveVoice.current = (event) => {
      if (event.type === 'started') {
        const input = currentVoiceEditor.current?.(event);
        if (input) {
          activeRecording.current = { id: event.recordingId, draft: draftRef.current, input };
        }
        return;
      }
      const recording = activeRecording.current;
      if (event.type === 'polishing') {
        if (recording?.id === event.recordingId) setVoicePolishing(true);
        return;
      }
      setVoicePolishing(false);
      if (event.type === 'cancelled') {
        if (recording?.id === event.recordingId) activeRecording.current = undefined;
        return;
      }
      void client
        .acknowledgeVoice?.(event.recordingId)
        .then((claimed) => {
          if (activeRecording.current !== recording || recording?.id !== event.recordingId) return;
          activeRecording.current = undefined;
          if (
            !claimed ||
            !event.text.trim() ||
            currentVoiceEditor.current?.(event) !== recording.input ||
            draftRef.current !== recording.draft
          )
            return;
          const offset = event.selectionEnd;
          const caret = offset + event.text.length;
          replaceDraft(
            recording.draft.slice(0, offset) + event.text + recording.draft.slice(offset),
            { start: caret, end: caret, direction: 'none' },
          );
        })
        .catch(() => {});
    };
  });

  useEffect(() => {
    let stopped = false;
    let stop: (() => void) | undefined;
    const clearRecording = () => {
      activeRecording.current = undefined;
      setVoicePolishing(false);
    };
    window.addEventListener('blur', clearRecording);
    void client
      .onVoiceEvent((event) => {
        if (!stopped) receiveVoice.current(event);
      })
      .then((unsubscribe) => {
        if (stopped) unsubscribe();
        else {
          stop = unsubscribe;
          setVoiceListening(true);
        }
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
      activeRecording.current = undefined;
      window.removeEventListener('blur', clearRecording);
      stop?.();
    };
  }, [client, notice]);

  useLayoutEffect(() => {
    if (!connected) {
      activeRecording.current = undefined;
      setVoicePolishing(false);
    }
  }, [connected]);

  const box = (
    <div
      className="composer"
      data-polishing={showPolish && !draft}
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
        placeholder={showPolish ? polishLabel : PLACEHOLDERS}
        status={turnId ? 'streaming' : 'idle'}
        efforts={EFFORTS}
        value={draft}
        inputRef={editor}
        submitDisabled={cannotSubmit || isComposing}
        canSubmitEmpty={attachments.length > 0 && attachmentsReady}
        inputProps={{
          'aria-label': PLACEHOLDERS[0],
          'aria-describedby': attachments.length ? 'draft-references' : undefined,
          readOnly: pending,
          onFocus: () => syncEditor(),
          onBlur: () => {
            activeRecording.current = undefined;
            setVoicePolishing(false);
            syncEditor(true);
          },
          onSelect: () => syncEditor(),
          onCompositionStart: () => {
            activeRecording.current = undefined;
            composing.current = true;
            syncEditor();
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
          if (value !== draftRef.current) activeRecording.current = undefined;
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
                errorMessage(cause, 'Could not stop the current request.'),
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
    </div>
  );
  return box;
}
