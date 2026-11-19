import { useMemo, type MouseEvent } from 'react';
import { CheckIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openExternal } from '../../api/files';
import { AssistantMarkdown } from './chat/markdown';
import { QaAnswerFooter } from './QaAnswerFooter';
import { qaSources } from './qaSources';

interface QaStreamingResponseProps {
  markdown: string;
  streaming?: boolean;
  elapsedSeconds?: number;
  activityLabel?: string;
  canReadAloud?: boolean;
}

function openAnswerLink(event: MouseEvent) {
  if (event.defaultPrevented || event.button !== 0 || !(event.target instanceof Element)) return;
  const anchor = event.target.closest('a');
  if (!anchor?.matches('[href^="http://" i], [href^="https://" i]')) return;
  event.preventDefault();
  if (anchor.username || anchor.password) return;
  void openExternal(anchor.href).catch(() => undefined);
}

export function QaStreamingResponse({
  markdown,
  streaming = false,
  elapsedSeconds,
  activityLabel,
  canReadAloud = false,
}: QaStreamingResponseProps) {
  const { t } = useTranslation();
  const answer = useMemo(() => (streaming ? null : qaSources(markdown)), [markdown, streaming]);
  const elapsed = Math.max(1, elapsedSeconds ?? 1);
  const label = streaming
    ? activityLabel || t('qa.thoughtLive', { seconds: elapsed })
    : t('qa.thoughtDone', { seconds: elapsed });

  return (
    <div className="qa-assistant-message" aria-label={t('qa.compact.answer')}>
      {(streaming || elapsedSeconds != null) && (
        <div className="qa-thought" role="status">
          {streaming ? <span className="qa-thinking-dot" /> : <CheckIcon aria-hidden />}
          {label}
        </div>
      )}
      {(answer?.body ?? markdown) && (
        <div onClick={openAnswerLink}>
          <AssistantMarkdown markdown={answer?.body ?? markdown} streaming={streaming} />
        </div>
      )}
      {answer && (
        <QaAnswerFooter
          text={markdown}
          sources={answer.sources}
          speechText={answer.body}
          canReadAloud={canReadAloud}
        />
      )}
    </div>
  );
}
