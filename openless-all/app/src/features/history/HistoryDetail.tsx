import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession, StylePack } from '../../api/contracts';
import type { Interaction } from '../../api/interaction';
import { LearnVocabulary } from '../../components/LearnVocabulary';
import { Hint } from '../../components/Tooltip';
import { AssistantMarkdown } from '../../components/chat/markdown';
import { canRetranscribeHistoryEntry } from '../../lib/history-retranscribe';
import { formatHistoryTime } from '../../lib/localeFormat';
import { getRuntimeOS } from '../../runtime/environment';
import { useHotkeySettings } from '../../data/preferences';
import { Button as Btn } from '../../ui/controls/Button';
import { Card } from '../../ui/layout/Card';
import { Pill } from '../../ui/controls/Pill';
import { AudioRecordingPlayer } from './AudioRecordingPlayer';
import { HistoryActionMenu } from './HistoryActionMenu';
import { RepolishPanel, HistoryResultCard } from './RepolishPanel';
import { formatDuration, formatStepDuration, TRUNCATED_PILL_STYLE } from './presentation';
import { useHistoryEntryActions } from './useHistoryEntryActions';

interface Props {
  item: DictationSession;
  mobile: boolean;
  onBack: () => void;
  quickNotesOnly: boolean;
  styleLabel: (entry: DictationSession) => string;
  allPacks: StylePack[] | null;
  packsError: string | null;
  confirm: Interaction['confirm'];
  notify: Interaction['notify'];
}

export function HistoryDetail({
  item,
  mobile,
  onBack,
  quickNotesOnly,
  styleLabel,
  allPacks,
  packsError,
  confirm,
  notify,
}: Props) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language;
  const os = getRuntimeOS();
  const { prefs } = useHotkeySettings();
  const [showRawTranscript, setShowRawTranscript] = useState(false);
  const [repolishOpen, setRepolishOpen] = useState(false);
  const [playbackRequest, setPlaybackRequest] = useState(0);
  const [audioLoading, setAudioLoading] = useState(false);
  const {
    audioMissing,
    handleAudioMissing,
    actionError,
    retranscribing,
    retranscriptionResult,
    justCopied,
    justCopiedRaw,
    onCopy,
    onExportAudio,
    onShareAudio,
    onRetranscribe,
    onDelete,
    onChooseExportDirectory,
    onResetExportDirectory,
  } = useHistoryEntryActions(item, quickNotesOnly, confirm, notify);

  return (
    <Card
      padding={20}
      className="ol-thinscroll ol-col-8"
      style={{ overflow: 'auto', minWidth: 0, minHeight: 0 }}
    >
      {mobile && (
        <div style={{ marginBottom: 12 }}>
          <Btn icon="chevLeft" variant="ghost" size="sm" onClick={onBack}>
            {t('history.backToList')}
          </Btn>
        </div>
      )}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 14,
          flexWrap: 'wrap',
          gap: 8,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
          <span
            style={{
              fontSize: 13,
              fontFamily: 'var(--ol-font-mono)',
              color: 'var(--ol-ink-3)',
              flexShrink: 0,
            }}
          >
            {formatHistoryTime(item.createdAt, locale)}
          </span>
          <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
            <Pill size="sm" tone="default" style={TRUNCATED_PILL_STYLE}>
              {styleLabel(item)}
            </Pill>
          </span>
          {item.pipelineMode === 'multimodal' && (
            <Pill size="sm" tone="blue">
              {t('history.multimodalPipeline')}
            </Pill>
          )}
          {/* "Recorded" prefix: distinct from the recognize/polish timings below —
                      recording time happens before the key release and must not be summed with
                      the pipeline steps (user feedback: "times don't add up"). */}
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {t('history.recorded', {
              duration: formatDuration(item.durationMs, t, locale),
            })}
          </span>
        </div>
        <HistoryActionMenu
          hasAudioRecording={item.hasAudioRecording === true && !audioMissing}
          audioLoading={audioLoading}
          showShare={os === 'android'}
          canRetranscribe={canRetranscribeHistoryEntry(item) && !audioMissing}
          retranscribing={retranscribing}
          canRepolish={Boolean(item.rawTranscript.trim() || quickNotesOnly)}
          showSaveDirectory={quickNotesOnly && os !== 'android'}
          exportDirectory={prefs?.quickNoteExportDirectory ?? ''}
          onPlay={() => setPlaybackRequest((request) => request + 1)}
          onExport={() => void onExportAudio()}
          onShare={() => void onShareAudio()}
          onRetranscribe={() => void onRetranscribe()}
          onDelete={onDelete}
          onRepolish={() => setRepolishOpen(true)}
          onChooseSaveDirectory={() => void onChooseExportDirectory()}
          onResetSaveDirectory={() => void onResetExportDirectory()}
        />
      </div>
      {/* The key must carry a component prefix: RepolishPanel below is a sibling at the
                  same level; if both used bare `item.id` the level would have duplicate keys.
                  React only warns (no error), but reconcile fails to match the old fiber — every
                  entry switch leaves a stale playback control in the DOM, and a long-open window
                  can stack up a whole column of them. */}
      {actionError && (
        <p role="alert" style={{ color: 'var(--ol-err)', fontSize: 12, marginBlock: '8px 12px' }}>
          {actionError}
        </p>
      )}
      {item.hasAudioRecording && !audioMissing && (
        <AudioRecordingPlayer
          sessionId={item.id}
          onMissing={handleAudioMissing}
          playRequest={playbackRequest}
          onLoadingChange={setAudioLoading}
          key={`audio-${item.id}`}
        />
      )}
      {retranscriptionResult?.sessionId === item.id && (
        <div style={{ marginBottom: 14 }}>
          <HistoryResultCard title={t('history.retranscribe')} text={retranscriptionResult.text} />
        </div>
      )}
      {/* Pipeline detail keeps only the recognize / polish steps — left column: step
                  name, middle column: provider·model, right column: step duration/status.
                  Insertion is a foreground-delivery detail and is not shown in the note content
                  area. */}
      <div
        className="ol-grid-12 ol-pipeline-grid"
        style={{
          marginBottom: 16,
          paddingBottom: 14,
          borderBottom: '0.5px solid var(--ol-line-soft)',
          rowGap: 7,
          fontSize: 11,
          color: 'var(--ol-ink-4)',
          alignItems: 'baseline',
        }}
      >
        {(item.asrProvider || item.asrMs != null) && (
          <>
            <span style={{ display: 'flex' }}>
              <Hint content={t('history.stepAsrHint')}>{t('history.stepAsr')}</Hint>
            </span>
            <span
              style={{
                color: 'var(--ol-ink-2)',
                fontFamily: 'var(--ol-font-mono)',
                overflowWrap: 'anywhere',
              }}
            >
              {[item.asrProvider, item.asrModel].filter(Boolean).join(' · ')}
            </span>
            <span
              style={{
                fontFamily: 'var(--ol-font-mono)',
                textAlign: 'right',
                whiteSpace: 'nowrap',
              }}
            >
              {item.asrMs != null ? formatStepDuration(item.asrMs, t, locale) : ''}
            </span>
          </>
        )}
        {(item.llmProvider || item.llmModel || item.polishMs != null) && (
          <>
            <span>{t('history.stepPolish')}</span>
            <span
              style={{
                color: 'var(--ol-ink-2)',
                fontFamily: 'var(--ol-font-mono)',
                overflowWrap: 'anywhere',
              }}
            >
              {[item.llmProvider, item.llmModel].filter(Boolean).join(' · ')}
            </span>
            <span
              style={{
                fontFamily: 'var(--ol-font-mono)',
                textAlign: 'right',
                whiteSpace: 'nowrap',
              }}
            >
              {item.polishMs != null ? formatStepDuration(item.polishMs, t, locale) : ''}
            </span>
          </>
        )}
      </div>
      {/* Default to showing only the polished result; the raw text can still be
                  expanded on demand, sparing users two duplicate columns every time. */}
      <div style={{ display: 'grid', gap: 12 }}>
        {/* The polish result box is de-blued too: neutral surface-2 fill + thin
                    outline. */}
        <div
          style={{
            minWidth: 0,
            padding: 14,
            border: '0.5px solid var(--ol-line)',
            borderRadius: 'var(--ol-r-md)',
            background: 'var(--ol-surface-2)',
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 8,
              flexWrap: 'wrap',
              marginBottom: 10,
            }}
          >
            <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
              <Pill size="sm" tone="blue" style={TRUNCATED_PILL_STYLE}>
                {t('history.stepPolish')} · {styleLabel(item)}
              </Pill>
            </span>
            <span style={{ display: 'inline-flex', gap: 6, flexShrink: 0 }}>
              {item.rawTranscript && (
                <Btn
                  variant="ghost"
                  size="sm"
                  onClick={() => setShowRawTranscript((visible) => !visible)}
                >
                  {showRawTranscript
                    ? t('history.hideRaw', '隐藏原文')
                    : t('history.showRaw', '查看原文')}
                </Btn>
              )}
              <Btn
                icon={justCopied ? 'check' : 'copy'}
                variant="ghost"
                size="sm"
                onClick={() => void onCopy(false)}
              >
                {justCopied ? t('common.copied') : t('common.copy')}
              </Btn>
            </span>
          </div>
          <div style={{ fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)' }}>
            <AssistantMarkdown
              markdown={
                item.finalText ||
                item.rawTranscript ||
                t('quickNote.noTranscript', 'No transcript yet.')
              }
            />
            <LearnVocabulary key={item.id} />
          </div>
        </div>
        {showRawTranscript && (
          <div
            style={{
              minWidth: 0,
              padding: 14,
              border: '0.5px solid var(--ol-line)',
              borderRadius: 'var(--ol-r-md)',
              background: 'var(--ol-surface-2)',
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 8,
                marginBottom: 10,
              }}
            >
              <Pill size="sm" tone="outline">
                {t('history.stepAsr')}
              </Pill>
              {item.rawTranscript && (
                <Btn
                  icon={justCopiedRaw ? 'check' : 'copy'}
                  variant="ghost"
                  size="sm"
                  onClick={() => void onCopy(true)}
                >
                  {justCopiedRaw ? t('common.copied') : t('common.copy')}
                </Btn>
              )}
            </div>
            <p
              style={{
                margin: 0,
                fontSize: 13,
                lineHeight: 1.7,
                color: 'var(--ol-ink-2)',
                whiteSpace: 'pre-wrap',
              }}
            >
              {item.rawTranscript || t('history.rawEmpty')}
            </p>
          </div>
        )}
      </div>
      {/* Repolish: run the LLM again on this entry's raw text. No raw text means
                  nothing to repolish (failed transcription entries) — the whole block is not
                  rendered; QA entries' raw text is the question, not text to polish, so they are
                  not rendered either. The key resets result and state when switching entries, so
                  the previous entry's result does not linger under the new one; the prefix
                  distinguishes it from the player's key above (duplicate same-level keys would
                  leave stale nodes). */}
      {repolishOpen &&
        (item.rawTranscript.trim() || quickNotesOnly) &&
        item.errorCode !== 'qaSession' && (
          <RepolishPanel
            session={item}
            mobile={mobile}
            allPacks={allPacks}
            packsError={packsError}
            onClose={() => setRepolishOpen(false)}
            persistOnApply={quickNotesOnly}
            key={`repolish-${item.id}`}
          />
        )}
    </Card>
  );
}
