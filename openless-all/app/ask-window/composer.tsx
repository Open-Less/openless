import { useCurrentAskNotice, useClearAskNotice, useAskNotice } from './notices';
import { PromptBox, type PromptEffort } from '@/components/ui/prompt-box';
import { Brain, Paperclip, Quote, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useInteraction } from '../src/ui/overlays/InteractionProvider';
import { askReferenceLabels } from '../src/i18n/askTurnReference';
import { openTurnReferenceEditor, openTurnReferencePreview } from './turn-reference';
import { AttachmentDeck } from './attachments';
import {
  askLocale,
  askIdleLabel,
  askErrorLabel,
  newRequestId,
  requestFingerprint,
  requiresNewAskConversation,
  type AskClient,
  type AskFocus,
  type AskSnapshot,
  type AskVoiceEvent,
  type SubmitAsk,
} from './client';
import { readDroppedImage } from './drop-image';

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
  result,
  captureSubmitReceipt,
  referenceBusy,
  referenceText,
  seedDraft,
  setReference,
  removeReference,
}: {
  client: AskClient;
  snapshot: AskSnapshot;
  connected: boolean;
  canDeliverVoice: () => boolean;
  check: () => void;
  focus?: AskFocus;
  activation: number;
  expanded: boolean;
  result?: ReactNode;
  captureSubmitReceipt: () => (turnId: string) => void;
  referenceBusy: boolean;
  referenceText?: string;
  seedDraft?: string;
  setReference: (text: string, draft: string) => void;
  removeReference: () => void;
}) {
  const { conversationId, attachments, turnReference } = snapshot;
  const { openDialog } = useInteraction(connected ? conversationId : undefined);
  const notice = useAskNotice();
  const clearNotice = useClearAskNotice();
  const currentNotice = useCurrentAskNotice();
  const cancelOnAccept = useRef(false);
  const lifetime = useRef(true);
  useEffect(() => {
    lifetime.current = true;
    return () => { lifetime.current = false; };
  }, []);
  const editor = useRef<HTMLTextAreaElement>(null);
  const draftRef = useRef(seedDraft ?? '');
  const seeded = useRef(seedDraft !== undefined);
  const lastClear = useRef<Draft>();
  const sentDraft = useRef<{ turnId: string; draft: Draft }>();
  const [recoverDraft, setRecoverDraft] = useState<Draft>();
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

  const [draft, setDraft] = useState(seedDraft ?? '');
  const [deepThinking, setDeepThinking] = useState(false);
  const [pending, setPending] = useState(false);
  const [acknowledgedTurn, setAcknowledgedTurn] = useState<string>();
  const [voiceListening, setVoiceListening] = useState(false);
  const [language, setLanguage] = useState(askLocale);
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<ReadonlySet<string>>(new Set());
  const [cancelling, setCancelling] = useState<string>();
  const [isComposing, setIsComposing] = useState(false);
  const [referenceRejected, setReferenceRejected] = useState(false);

  const turnId =
    snapshot.activeTurnId ??
    (acknowledgedTurn &&
    snapshot.submission?.currentTurnId !== acknowledgedTurn &&
    !snapshot.turns.some((turn) => turn.turnId === acknowledgedTurn)
      ? acknowledgedTurn
      : undefined);
  const attachmentsReady = attachments.every((item) => item.state === 'ready');
  const submission = snapshot.submission;
  const referenceUsed = !!turnReference && (referenceRejected || turnReference.active === true ||
    !!submission || !!acknowledgedTurn || snapshot.turns.length > 0);
  const referenceLabels = askReferenceLabels(language);
  const cannotSubmit = !connected || pending || adding || removing.size > 0 || !attachmentsReady ||
    referenceBusy || referenceUsed;
  const current = submission?.currentTurnId === snapshot.activeTurnId ? submission : undefined;
  const showPolish = current?.route === 'edit' && current.status === 'polishing';
  const showWorking = current && (!current.route || current.route === 'write' || current.route === 'edit')
    && (current.status === 'thinking' || current.status === 'writing');
  const idleLabel = askIdleLabel(language);
  const polishLabel = /^zh-(TW|HK|Hant)\b/i.test(language)
    ? '正在潤飾'
    : /^zh\b/i.test(language)
      ? '正在润色'
      : 'Polishing…';
  const workingLabel = /^zh-(TW|HK|Hant)\b/i.test(language) ? '處理中…'
    : /^zh\b/i.test(language) ? '处理中…' : 'Working…';
  const placeholder = !expanded && currentNotice ? askErrorLabel(currentNotice.code, language)
    : !draft && showPolish ? polishLabel
    : !draft && showWorking ? workingLabel : idleLabel;
  const showDraftError = !expanded && Boolean(currentNotice) && draft.length > 0;

  useEffect(() => {
    const update = () => setLanguage(askLocale());
    window.addEventListener('storage', update);
    window.addEventListener('languagechange', update);
    window.addEventListener('focus', update);
    return () => {
      window.removeEventListener('storage', update);
      window.removeEventListener('languagechange', update);
      window.removeEventListener('focus', update);
    };
  }, []);

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
    clearNotice();
    draftRef.current = text;
    setDraft(text);
  }

  function restoreDraft(draft: Draft) {
    if (!draftRef.current) replaceDraft(draft.text, draft.selection);
    else setRecoverDraft(draft);
  }

  useLayoutEffect(() => {
    if (seedDraft === undefined || seeded.current) return;
    seeded.current = true;
    if (draftRef.current || sentDraft.current || pendingRef.current) return;
    draftRef.current = seedDraft;
    setDraft(seedDraft);
  }, [seedDraft]);

  useLayoutEffect(() => {
    const sent = sentDraft.current;
    if (!sent || submission?.currentTurnId !== sent.turnId || snapshot.activeTurnId === sent.turnId) return;
    sentDraft.current = undefined;
    const writeUnconfirmed = submission.delivery === 'retained' || submission.delivery === 'unknown';
    if (!writeUnconfirmed && submission.status !== 'failed' && submission.status !== 'cancelled'
      && submission.route !== 'clarify') return;
    restoreDraft(sent.draft);
  }, [acknowledgedTurn, submission, snapshot.activeTurnId]);

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
      referenceBusy ||
      referenceUsed ||
      !!turnId ||
      adding ||
      request.attachmentIds.some((attachmentId) => !attachmentIds.has(attachmentId));
    if (blocked) {
      if (restore) replaceDraft(restore.text, restore.selection);
      return;
    }

    clearNotice();
    cancelOnAccept.current = false;
    pendingRef.current = true;
    const reportAccepted = captureSubmitReceipt();
    setPending(true);
    void client
      .submit(request)
      .then(({ turnId: acceptedTurnId }) => {
        if (cancelOnAccept.current) {
          void client.cancel(request.conversationId, acceptedTurnId).catch(() => {});
          if (restore && lifetime.current) restoreDraft(restore);
          return;
        }
        if (!lifetime.current) return;
        if (restore) sentDraft.current = { turnId: acceptedTurnId, draft: restore };
        setAcknowledgedTurn(acceptedTurnId);
        reportAccepted(acceptedTurnId);
        draftIds.current.delete(requestFingerprint(request));
      })
      .catch((cause) => {
        if (!lifetime.current) return;
        if (cancelOnAccept.current) {
          if (restore) restoreDraft(restore);
          return;
        }
        const recoverable =
          typeof cause === 'object' &&
          cause !== null &&
          'code' in cause &&
          ['busy', 'invalid_argument', 'cancelled'].includes(String(cause.code));
        const severity = recoverable ? 'warning' : 'error';
        if (restore) restoreDraft(restore);
        if (requiresNewAskConversation(cause)) setReferenceRejected(true);
        notice(cause, severity, 'ask-send');
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
        notice(cause, 'warning', 'ask-attachment'),
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
      if (recording?.id !== event.recordingId) return;
      if (event.type === 'cancelled' || event.type === 'failed') {
        activeRecording.current = undefined;
        if (event.type === 'failed' && currentVoiceEditor.current?.(event) === recording.input) {
          notice(undefined, 'error', 'ask-voice');
        }
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
            cause,
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
    }
  }, [connected]);

  async function addReference() {
    const text = await openTurnReferenceEditor(
      openDialog,
      referenceLabels,
      !!submission || snapshot.turns.length > 0 || attachments.length > 0,
    );
    if (text !== undefined && lifetime.current && !pendingRef.current) setReference(text, draftRef.current);
  }

  async function previewReference() {
    if (!turnReference) return;
    try {
      const validated = await client.snapshot(conversationId);
      const reference = validated.turnReference;
      if (!reference || reference.source !== turnReference.source || validated.conversationId !== conversationId) {
        if (lifetime.current) notice({ code: 'askReferenceUnavailable' }, 'warning');
        check();
        return;
      }
      if (lifetime.current) {
        await openTurnReferencePreview(
          openDialog,
          referenceLabels,
          referenceLabels[reference.source],
          reference.text ?? referenceText,
        );
      }
    } catch (error) {
      if (lifetime.current) notice(error, 'warning');
      check();
    }
  }

  const noticeLabel = currentNotice && askErrorLabel(currentNotice.code, language);

  const box = (
    <div
      className="composer"
      data-polishing={showPolish && !draft && !currentNotice}
      data-error-code={!expanded ? currentNotice?.code : undefined}
      data-draft-error={showDraftError || undefined}
      onDragOver={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = connected && !adding && !pending && !referenceBusy && !referenceUsed ? 'copy' : 'none';
      }}
      onDrop={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        if (!connected || adding || pendingRef.current || referenceBusy || referenceUsed) return;
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
      {!expanded && currentNotice && <span className="sr-only" role="alert">{askErrorLabel(currentNotice.code, language)}</span>}
      <PromptBox
        className="w-full"
        placeholder={placeholder}
        status={turnId ? 'streaming' : 'idle'}
        efforts={EFFORTS}
        value={draft}
        inputRef={editor}
        submitDisabled={cannotSubmit || isComposing}
        canSubmitEmpty={attachments.length > 0 && attachmentsReady}
        inputProps={{
          'aria-label': idleLabel,
          'aria-describedby': attachments.length ? 'draft-references' : undefined,
          readOnly: pending || referenceBusy || referenceUsed,
          onKeyDown: (event) => {
            if (showDraftError && !['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) clearNotice();
            if (event.key === 'Escape' && !event.nativeEvent.isComposing && !composing.current && pendingRef.current) {
              cancelOnAccept.current = true;
            }
          },
          onPointerDown: () => {
            if (showDraftError) clearNotice();
          },
          onFocus: () => syncEditor(),
          onBlur: () => {
            activeRecording.current = undefined;
            syncEditor(true);
          },
          onSelect: () => syncEditor(),
          onCompositionStart: () => {
            clearNotice();
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
          if (pendingRef.current || referenceBusy || referenceUsed) return;
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
          clearNotice();
          setCancelling(turnId);
          void client
            .cancel(conversationId, turnId)
            .catch((cause) => {
              notice(
                cause,
                'error',
                'ask-stop',
              );
            })
            .finally(() => {
              setCancelling(undefined);
              check();
            });
        }}
        beforeInput={<>
          {result}
          {turnReference && (
            <div className="ask-reference-row">
              <button
                type="button"
                className="ask-reference-chip"
                disabled={referenceBusy || !connected}
                aria-label={`${referenceLabels.title} · ${referenceLabels[turnReference.source]}`}
                onClick={() => void previewReference()}
              >
                <Quote size={14} aria-hidden="true" />
                {referenceLabels[turnReference.source]}
              </button>
              <ActionButton label={referenceLabels.remove} disabled={referenceBusy || !connected}
                onClick={removeReference}>
                <X size={16} aria-hidden="true" />
              </ActionButton>
              {referenceUsed && !turnId && (
                <button type="button" className="ask-reference-chip" disabled={referenceBusy || !connected}
                  onClick={removeReference}>
                  {referenceLabels.fresh}
                </button>
              )}
            </div>
          )}
          {recoverDraft && <details className="ask-inline-notice">
            <summary>Previous instruction</summary>
            <p className="result-text">{recoverDraft.text}</p>
          </details>}
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
                    cause,
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
          {showDraftError && currentNotice && (
            <div className="ask-draft-error-anchor" aria-hidden="true">
              <span className="ask-draft-error">
                {noticeLabel?.endsWith(currentNotice.code) ? <>
                  <span>{noticeLabel.slice(0, -currentNotice.code.length)}</span>
                  <span>{currentNotice.code}</span>
                </> : <span>{noticeLabel}</span>}
              </span>
            </div>
          )}
        </>}
        actions={
          <>
            <ActionButton
              label="Attach a file"
              disabled={adding || pending || !connected || referenceBusy || referenceUsed}
              onClick={() => attach(() => client.addAttachment(conversationId))}
            >
              <Paperclip size={18} />
            </ActionButton>
            {!turnReference && (
              <ActionButton
                label={referenceLabels.add}
                disabled={referenceBusy || !!turnId || adding || pending || removing.size > 0 || !connected}
                onClick={() => void addReference()}
              >
                <Quote size={18} />
              </ActionButton>
            )}
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
