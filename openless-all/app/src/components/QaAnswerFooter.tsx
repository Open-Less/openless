import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CheckIcon, CopyIcon, GlobeIcon } from 'lucide-react';
import { DeepSeekReadAloud } from './DeepSeekReadAloud';
import { openExternal } from '../lib/ipc';
import { isTauri } from '../lib/ipc/shared';
import type { QaSource } from '../lib/qaSources';

function SourceLink({ source, index }: { source: QaSource; index: number }) {
  const [failed, setFailed] = useState(false);
  return (
    <a
      className="qa-source-link"
      href={source.url}
      title={`${source.title} · ${source.host}`}
      aria-label={`${index + 1}. ${source.title} (${source.host})`}
      style={{ animationDelay: `${index * 35}ms` }}
      onClick={(event) => {
        event.preventDefault();
        void openExternal(source.url);
      }}
    >
      {isTauri && !failed ? (
        <img
          src={`https://icons.duckduckgo.com/ip3/${encodeURIComponent(source.host)}.ico`}
          referrerPolicy="no-referrer"
          alt=""
          onError={() => setFailed(true)}
        />
      ) : (
        <GlobeIcon aria-hidden size={12} />
      )}
      <span>{index + 1}</span>
    </a>
  );
}
export function QaAnswerFooter({
  text,
  sources,
  canReadAloud,
  speechText,
}: {
  text: string;
  sources: QaSource[];
  canReadAloud: boolean;
  speechText?: string;
}) {
  const { t, i18n } = useTranslation();
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1000);
    return () => clearTimeout(timer);
  }, [copied]);
  return (
    <div className="qa-answer-footer">
      <div
        className="qa-answer-sources"
        aria-label={t('qa.sources', {
          defaultValue: i18n.language.startsWith('zh') ? '来源' : 'Sources',
        })}
      >
        {sources.map((source, index) => (
          <SourceLink key={source.url} source={source} index={index} />
        ))}
      </div>
      <div className="qa-answer-actions">
        <button
          type="button"
          className="qa-copy"
          data-copied={copied}
          aria-label={t('common.copy')}
          onClick={() => {
            void navigator.clipboard
              .writeText(text)
              .then(() => setCopied(true))
              .catch(() => setCopied(false));
          }}
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
        </button>
        {canReadAloud && <DeepSeekReadAloud text={speechText ?? text} />}
      </div>
    </div>
  );
}
