import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { DictationSession, StylePack } from '../../api/contracts';
import type { Interaction } from '../../api/interaction';
import { Hint } from '../../components/Tooltip';
import { AssistantMarkdown } from '../../components/chat/markdown';
import { canRetranscribeHistoryEntry } from '../../lib/history-retranscribe';
import { formatHistoryTime } from '../../lib/localeFormat';
import { useReducedMotion } from '../../lib/motion';
import { getRuntimeOS } from '../../runtime/environment';
import { useHotkeySettings } from '../../data/preferences';
import { Button as Btn } from '../../ui/controls/Button';
import { Segmented } from '../../ui/controls/Segmented';
import { Card } from '../../ui/layout/Card';
import { Pill } from '../../ui/controls/Pill';
import { AudioRecordingPlayer } from './AudioRecordingPlayer';
import { HistoryActionMenu } from './HistoryActionMenu';
import { RepolishPanel, HistoryResultCard } from './RepolishPanel';
import { formatDuration, formatStepDuration, TRUNCATED_PILL_STYLE } from './presentation';
import { useHistoryEntryActions } from './useHistoryEntryActions';
import './history.css';

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
  const [contentView, setContentView] = useState<'polished' | 'raw'>('polished');
  const contentScroll = useRef<HTMLDivElement>(null);
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

  const hasRaw = Boolean(item.rawTranscript);
  const activeView = hasRaw ? contentView : 'polished';
  const showingRaw = activeView === 'raw';
  const activeCopied = showingRaw ? justCopiedRaw : justCopied;
  const hasAsr = Boolean(item.asrProvider || item.asrMs != null);
  const hasPolish = Boolean(item.llmProvider || item.llmModel || item.polishMs != null);
  const polishSummaryLabel = `${t('history.stepPolish')} · ${styleLabel(item)}`;

  useLayoutEffect(() => {
    if (contentScroll.current) contentScroll.current.scrollTop = 0;
  }, [activeView]);

  return (
    <Card
      padding={16}
      className="ol-history-detail-card ol-col-8"
      data-mobile={mobile || undefined}
    >
      {mobile && (
        <div style={{ marginBottom: 10, flexShrink: 0 }}>
          <Btn icon="chevLeft" variant="ghost" size="sm" onClick={onBack}>
            {t('history.backToList')}
          </Btn>
        </div>
      )}
      <div className="ol-history-detail-header">
        <div className="ol-history-detail-meta">
          <span className="ol-history-detail-time">
            {formatHistoryTime(item.createdAt, locale)}
          </span>
          <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
            <Pill size="sm" tone="outline" style={TRUNCATED_PILL_STYLE}>
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
          <span className="ol-history-detail-recorded">
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
        <p role="alert" className="ol-history-detail-error">
          {actionError}
        </p>
      )}
      {item.hasAudioRecording && !audioMissing && (
        <div className="ol-history-detail-aux">
          <AudioRecordingPlayer
            sessionId={item.id}
            onMissing={handleAudioMissing}
            playRequest={playbackRequest}
            onLoadingChange={setAudioLoading}
            key={`audio-${item.id}`}
          />
        </div>
      )}
      {retranscriptionResult?.sessionId === item.id && (
        <div
          className="ol-history-retranscription ol-thinscroll"
          role="region"
          aria-label={t('history.retranscribe')}
          tabIndex={0}
        >
          <HistoryResultCard title={t('history.retranscribe')} text={retranscriptionResult.text} />
        </div>
      )}
      {(hasAsr || hasPolish) && (
        <div
          className="ol-history-pipeline-row"
          data-single={hasAsr !== hasPolish ? 'true' : undefined}
        >
          {hasAsr && (
            <PipelineStepChip
              label={<Hint content={t('history.stepAsrHint')}>{t('history.stepAsr')}</Hint>}
              value={[item.asrProvider, item.asrModel].filter(Boolean).join(' · ')}
              duration={item.asrMs != null ? formatStepDuration(item.asrMs, t, locale) : ''}
            />
          )}
          {hasPolish && (
            <PipelineStepChip
              label={t('history.stepPolish')}
              value={[item.llmProvider, item.llmModel].filter(Boolean).join(' · ')}
              duration={item.polishMs != null ? formatStepDuration(item.polishMs, t, locale) : ''}
            />
          )}
        </div>
      )}
      <div className="ol-history-content-frame">
        <div className="ol-history-content-header">
          {hasRaw ? (
            <Segmented
              className="ol-history-view-segmented"
              label={t('history.showRaw', '查看原文')}
              value={activeView}
              onChange={setContentView}
              compact
              options={[
                { value: 'polished', label: polishSummaryLabel, title: polishSummaryLabel },
                { value: 'raw', label: t('history.rawLabel', '原文') },
              ]}
            />
          ) : (
            <span style={{ display: 'flex', minWidth: 0 }} title={styleLabel(item)}>
              <Pill size="sm" tone="blue" style={TRUNCATED_PILL_STYLE}>
                {polishSummaryLabel}
              </Pill>
            </span>
          )}
        </div>
        <div ref={contentScroll} className="ol-history-content-scroll ol-thinscroll" tabIndex={0}>
          {showingRaw ? (
            <div className="ol-history-content-pane ol-history-content-pane--raw">
              <p className="ol-history-raw-text">{item.rawTranscript || t('history.rawEmpty')}</p>
            </div>
          ) : (
            <div className="ol-history-content-pane">
              <AssistantMarkdown
                markdown={
                  item.finalText ||
                  item.rawTranscript ||
                  t('quickNote.noTranscript', 'No transcript yet.')
                }
              />
            </div>
          )}
        </div>
        <div className="ol-history-content-footer">
          <Btn
            className="ol-history-copy-btn"
            icon={activeCopied ? 'check' : 'copy'}
            variant="ghost"
            size="sm"
            onClick={() => void onCopy(showingRaw)}
          >
            {activeCopied ? t('common.copied') : t('common.copy')}
          </Btn>
        </div>
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
          <div className="ol-history-repolish-wrap ol-thinscroll">
            <RepolishPanel
              session={item}
              mobile={mobile}
              allPacks={allPacks}
              packsError={packsError}
              onClose={() => setRepolishOpen(false)}
              persistOnApply={quickNotesOnly}
              key={`repolish-${item.id}`}
            />
          </div>
        )}
    </Card>
  );
}

function PipelineStepChip({
  label,
  value,
  duration,
}: {
  label: ReactNode;
  value: string;
  duration: string;
}) {
  return (
    <div className="ol-history-pipeline-chip">
      <span className="ol-history-pipeline-label">{label}</span>
      <OverflowMarqueeText text={value || '—'} />
      {duration && <span className="ol-history-pipeline-duration">{duration}</span>}
    </div>
  );
}

/** Auto-scrolls overflowing provider/model text horizontally and pauses cleanly on hover. */
function OverflowMarqueeText({ text }: { text: string }) {
  const reduced = useReducedMotion();
  const viewportRef = useRef<HTMLSpanElement>(null);
  const stripRef = useRef<HTMLSpanElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const [paused, setPaused] = useState(false);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    const track = stripRef.current;
    const measureNode = textRef.current;
    if (!viewport || !track || !measureNode || reduced || typeof track.animate !== 'function') {
      if (viewport) delete viewport.dataset.scrolling;
      return;
    }

    let animation: Animation | undefined;
    let lastWidth = -1;
    let lastViewport = -1;
    const reset = () => {
      animation?.cancel();
      animation = undefined;
      delete viewport.dataset.scrolling;
    };
    const sync = () => {
      const textWidth = measureNode.offsetWidth;
      const boxWidth = viewport.clientWidth;
      if (textWidth <= boxWidth + 1 || boxWidth === 0) {
        lastWidth = -1;
        lastViewport = -1;
        reset();
        return;
      }
      if (textWidth === lastWidth && boxWidth === lastViewport && animation) return;
      lastWidth = textWidth;
      lastViewport = boxWidth;
      animation?.cancel();
      viewport.dataset.scrolling = '';
      const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
      const sign = getComputedStyle(viewport).direction === 'rtl' ? 1 : -1;
      const end = `translateX(calc(${sign * 50}% + ${(sign * gap) / 2}px))`;
      animation = track.animate([{ transform: 'translateX(0px)' }, { transform: end }], {
        duration: Math.max(3200, ((textWidth + gap) / 42) * 1000),
        iterations: Infinity,
        easing: 'linear',
      });
      if (paused) animation.pause();
    };
    sync();
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(sync) : undefined;
    observer?.observe(viewport);
    observer?.observe(measureNode);
    return () => {
      observer?.disconnect();
      reset();
    };
  }, [text, reduced]);

  useLayoutEffect(() => {
    const track = stripRef.current;
    if (!track || typeof track.getAnimations !== 'function') return;
    for (const animation of track.getAnimations()) {
      if (paused) animation.pause();
      else animation.play();
    }
  }, [paused]);

  return (
    <span
      ref={viewportRef}
      className="ol-history-marquee"
      title={text}
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={() => setPaused(false)}
    >
      <span ref={stripRef} className="ol-history-marquee-strip">
        <span ref={textRef} className="ol-history-marquee-text">
          {text}
        </span>
        <span className="ol-history-marquee-copy" aria-hidden="true" data-text={text} />
      </span>
    </span>
  );
}
