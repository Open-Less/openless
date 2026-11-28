import { useEffect, useId, useState } from 'react';
import type { DialogActions, Interaction } from '../src/api/interaction';
import { askReferenceLabels } from '../src/i18n/askTurnReference';
import { DialogAction } from '../src/ui/overlays/Dialog';

type Labels = ReturnType<typeof askReferenceLabels>;

export function openTurnReferenceEditor(
  openDialog: Interaction['openDialog'],
  labels: Labels,
  hasConversationContent: boolean,
) {
  return openDialog<string>({
    title: labels.add,
    width: 'min(480px, 100%)',
    render: (actions) => <ReferenceEditor {...{ actions, labels, hasConversationContent }} />,
  });
}

function ReferenceEditor({
  labels,
  hasConversationContent,
  actions,
}: {
  labels: Labels;
  hasConversationContent: boolean;
  actions: DialogActions<string>;
}) {
  const inputId = useId();
  const [text, setText] = useState('');
  const tooLong = text.length > 8000 || [...text].length > 4000;
  return (
    <div className="ask-reference-dialog">
      <label htmlFor={inputId}>{labels.text}</label>
      <textarea
        id={inputId}
        rows={3}
        value={text}
        aria-invalid={tooLong || undefined}
        onChange={(event) => setText(event.target.value)}
      />
      {tooLong && <div className="ask-reference-error" role="alert">{labels.askReferenceLimit}</div>}
      <div className="ask-reference-actions">
        <DialogAction tone="neutral" onClick={actions.cancel}>{labels.cancel}</DialogAction>
        <DialogAction disabled={!text.trim() || tooLong} onClick={() => actions.resolve(text)}>
          {hasConversationContent ? labels.startWithReference : labels.attach}
        </DialogAction>
      </div>
    </div>
  );
}

export async function openTurnReferencePreview(
  openDialog: Interaction['openDialog'],
  labels: Labels,
  source: string,
  text?: string,
) {
  const lifetime = new AbortController();
  try {
    await openDialog<void>({
      title: labels.title,
      width: 'min(480px, 100%)',
      render: (actions) => (
        <ReferencePreview {...{ labels, source, text, actions }} signal={lifetime.signal} />
      ),
    });
  } finally {
    lifetime.abort();
  }
}

function ReferencePreview({
  labels,
  source,
  text,
  signal,
  actions,
}: {
  labels: Labels;
  source: string;
  text?: string;
  signal: AbortSignal;
  actions: DialogActions<void>;
}) {
  const [closed, setClosed] = useState(signal.aborted);
  useEffect(() => {
    const close = () => setClosed(true);
    signal.addEventListener('abort', close, { once: true });
    return () => signal.removeEventListener('abort', close);
  }, [signal]);
  return (
    <div className="ask-reference-dialog">
      <span>{source}</span>
      {!closed && (text
        ? <pre tabIndex={0} aria-label={labels.text}>{text}</pre>
        : <div role="status">{labels.askReferenceUnavailable}</div>)}
      <div className="ask-reference-actions">
        <DialogAction tone="neutral" onClick={() => actions.resolve()}>{labels.close}</DialogAction>
      </div>
    </div>
  );
}
