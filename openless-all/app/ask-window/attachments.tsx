import { FileText, Image as ImageIcon, Quote, X } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { errorMessage, type AskClient, type AttachmentInfo } from './client';

function AttachmentCard({
  client,
  conversationId,
  attachment,
  label,
  removing,
  disabled,
  remove,
}: {
  client: AskClient;
  conversationId: string;
  attachment: AttachmentInfo;
  label: string;
  removing: boolean;
  disabled: boolean;
  remove: () => void;
}) {
  const { attachmentId, kind, displayName, state, preview, error } = attachment;
  const [image, setImage] = useState<string>();
  const [expanded, setExpanded] = useState(false);
  const [fullText, setFullText] = useState<string>();
  const [failure, setFailure] = useState<string>();
  const [loading, setLoading] = useState(false);
  const hasThumbnail = preview?.kind === 'image' && preview.thumbnailAvailable;
  const excerpt = preview?.kind === 'selection' ? preview : undefined;
  const lifetime = useRef(false);

  useEffect(() => {
    lifetime.current = true;
    return () => {
      lifetime.current = false;
    };
  }, []);

  useEffect(() => {
    if (kind !== 'image' || state !== 'ready' || !hasThumbnail) return;
    let current = true;
    void client
      .getAttachmentPreview(conversationId, attachmentId)
      .then((result) => {
        if (!current) return;
        if (result?.kind !== 'image' || !['image/png', 'image/jpeg'].includes(result.mimeType)) {
          throw new Error('This thumbnail is not a supported image.');
        }
        setImage(`data:${result.mimeType};base64,${result.dataBase64}`);
      })
      .catch((cause) => {
        if (current) setFailure(errorMessage(cause, 'Could not load the thumbnail.'));
      });
    return () => {
      current = false;
    };
  }, [client, conversationId, attachmentId, kind, state, hasThumbnail]);

  async function toggleExcerpt() {
    setExpanded(!expanded);
    if (expanded || fullText !== undefined || loading) return;
    setLoading(true);
    setFailure(undefined);
    try {
      const result = await client.getAttachmentPreview(conversationId, attachmentId);
      if (result?.kind !== 'selection') throw new Error('The original selection is unavailable.');
      if (lifetime.current) setFullText(result.text);
    } catch (cause) {
      if (lifetime.current)
        setFailure(errorMessage(cause, 'Could not load the original selection.'));
    } finally {
      if (lifetime.current) setLoading(false);
    }
  }

  return (
    <article
      className="attachment-card"
      data-kind={kind}
      data-thumbnail={Boolean(image && !failure)}
      aria-label={label}
    >
      <button
        className="icon-button attachment-remove"
        type="button"
        aria-label={`Remove ${label}`}
        disabled={removing || disabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={remove}
      >
        <X size={13} />
      </button>
      {kind === 'image' ? (
        <>
          {image && !failure && (
            <img
              src={image}
              alt={displayName}
              onError={() => setFailure('Could not display the thumbnail.')}
            />
          )}
          {!image && state === 'processing' && <ImageIcon size={20} aria-hidden="true" />}
          <strong className="image-label">{label}</strong>
        </>
      ) : (
        <div className="attachment-heading">
          {kind === 'selection' ? (
            <Quote size={17} aria-hidden="true" />
          ) : (
            <FileText size={19} aria-hidden="true" />
          )}
          <div className="attachment-title">
            <strong title={kind === 'pdf' ? displayName : undefined}>
              {kind === 'selection' ? 'Selected text' : displayName}
            </strong>
            {kind === 'pdf' && (
              <small>PDF{attachment.pageCount ? ` · ${attachment.pageCount} pages` : ''}</small>
            )}
            {kind === 'selection' && attachment.sourceApp && <small>{attachment.sourceApp}</small>}
          </div>
        </div>
      )}
      {excerpt && (
        <>
          <blockquote className="selection-excerpt">
            {expanded && fullText !== undefined ? fullText : excerpt.text}
          </blockquote>
          {excerpt.hasMore && (
            <button
              className="text-button"
              type="button"
              aria-expanded={expanded}
              onClick={() => void toggleExcerpt()}
            >
              {expanded ? 'Show excerpt' : 'Read original'}
            </button>
          )}
          {expanded && loading && <small role="status">Loading the original…</small>}
        </>
      )}
      {state === 'processing' && <small role="status">Preparing…</small>}
      {state === 'error' && (
        <small className="problem">{error?.message || 'Attachment could not be prepared.'}</small>
      )}
      {state === 'ready' && kind === 'image' && !hasThumbnail && (
        <small className="problem">Thumbnail unavailable.</small>
      )}
      {state === 'ready' && kind === 'image' && hasThumbnail && !image && !failure && (
        <small role="status">Loading thumbnail…</small>
      )}
      {failure && (
        <small className="problem" role="status">
          {failure}
        </small>
      )}
    </article>
  );
}

export function AttachmentDeck({
  client,
  conversationId,
  attachments,
  removing,
  disabled,
  remove,
}: {
  client: AskClient;
  conversationId: string;
  attachments: AttachmentInfo[];
  removing: ReadonlySet<string>;
  disabled: boolean;
  remove: (attachmentId: string) => void;
}) {
  const numbers = useRef(new Map<string, number>());
  const labels = attachments.map((attachment) => {
    if (attachment.kind === 'pdf') return `PDF: ${attachment.displayName}`;
    if (attachment.kind === 'selection') return 'Selected text';
    if (!numbers.current.has(attachment.attachmentId)) {
      numbers.current.set(attachment.attachmentId, numbers.current.size + 1);
    }
    return `Image #${numbers.current.get(attachment.attachmentId)}`;
  });

  if (!attachments.length) return null;
  return (
    <>
      <div className="attachment-deck" aria-label="Draft attachments">
        {attachments.map((attachment, index) => (
          <AttachmentCard
            key={attachment.attachmentId}
            client={client}
            conversationId={conversationId}
            label={labels[index]}
            attachment={attachment}
            disabled={disabled}
            removing={removing.has(attachment.attachmentId)}
            remove={() => remove(attachment.attachmentId)}
          />
        ))}
      </div>
      <ul
        id="draft-references"
        className="attachment-references"
        aria-label="Attachment references"
      >
        {attachments.map((attachment, index) => (
          <li key={attachment.attachmentId} title={labels[index]}>
            {labels[index]}
          </li>
        ))}
      </ul>
    </>
  );
}
