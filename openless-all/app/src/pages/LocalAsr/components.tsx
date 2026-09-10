// Presentational sub-components for the Local ASR page, extracted from
// LocalAsr/index.tsx. Native state/actions stay in the parent; the catalog owns
// only its engine filter, narrow-layout navigation and keyboard focus.

import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import {
  type FoundryPrepareProgress,
  type HfModelCard,
  type LocalAsrDownloadProgress,
  type LocalAsrModelStatus,
  type LocalAsrTestResult,
} from '../../lib/localAsr';
import { Btn, Card, Pill } from '../_atoms';
import { Icon } from '../../components/Icon';
import { formatBytes } from './helpers';
import type { RemoteSize } from './types';
import { useLayoutStack } from '../../lib/useMobileLayout';
import './local-asr.css';

export function FoundryPrepareProgressBlock({
  progress,
  modelCached,
  cancelRequested,
}: {
  progress: FoundryPrepareProgress | null;
  modelCached: boolean;
  cancelRequested: boolean;
}) {
  const { t } = useTranslation();
  const stages = [
    { phase: 'runtime', label: t('localAsr.foundryPrepareRuntime') },
    { phase: 'model', label: t('localAsr.foundryPrepareModel') },
    { phase: 'load', label: t('localAsr.foundryPrepareLoad') },
  ] as const;
  const currentIndex = progress ? stages.findIndex((stage) => stage.phase === progress.phase) : -1;

  return (
    <div
      style={{
        padding: '10px 12px',
        borderRadius: 8,
        background: 'rgba(0,0,0,0.035)',
        display: 'flex',
        flexDirection: 'column',
        gap: 9,
      }}
    >
      {stages.map((stage, index) => {
        const finished = progress?.phase === 'finished' || currentIndex > index;
        const skippedCachedModel =
          stage.phase === 'model' &&
          modelCached &&
          (progress?.phase === 'load' || progress?.phase === 'finished');
        const active = progress?.phase === stage.phase;
        const failed = progress?.phase === 'failed';
        const percent =
          finished || skippedCachedModel
            ? 100
            : active
              ? Math.max(0, Math.min(100, progress?.percent ?? 0))
              : 0;
        const detail = skippedCachedModel
          ? t('localAsr.foundryPrepareModelSkipped')
          : active
            ? progress?.label
            : finished
              ? t('localAsr.foundryPrepareDone')
              : t('localAsr.foundryPrepareWaiting');
        return (
          <div key={stage.phase}>
            <div
              style={{
                display: 'flex',
                justifyContent: 'space-between',
                gap: 12,
                marginBottom: 5,
              }}
            >
              <span
                style={{
                  fontSize: 12,
                  color: 'var(--ol-ink-2)',
                  fontWeight: 600,
                }}
              >
                {stage.label}
              </span>
              <span
                style={{
                  fontSize: 11,
                  color: 'var(--ol-ink-4)',
                }}
              >
                {failed ? t('localAsr.failed') : `${Math.round(percent)}%`}
              </span>
            </div>
            <div
              style={{
                height: 6,
                borderRadius: 3,
                overflow: 'hidden',
                background: 'rgba(0,0,0,0.08)',
              }}
            >
              <div
                style={{
                  height: '100%',
                  width: `${percent}%`,
                  background: failed ? '#d04545' : 'var(--ol-accent-blue, #2c5cff)',
                  transition: 'width 120ms linear',
                }}
              />
            </div>
            <div
              style={{
                fontSize: 11,
                color: 'var(--ol-ink-4)',
                marginTop: 4,
              }}
            >
              {detail}
            </div>
          </div>
        );
      })}
      {cancelRequested && (
        <div
          style={{
            fontSize: 11.5,
            color: '#8a5a00',
            lineHeight: 1.5,
          }}
        >
          {t('localAsr.foundryCancelBestEffort')}
        </div>
      )}
      {progress?.phase === 'failed' && progress.error && (
        <div
          style={{
            fontSize: 11.5,
            color: '#9b2c2c',
            lineHeight: 1.5,
          }}
        >
          {progress.error}
        </div>
      )}
    </div>
  );
}

export function DownloadProgressBlock({
  progress,
  remoteSize,
  cancelRequested,
}: {
  progress?: LocalAsrDownloadProgress;
  remoteSize?: RemoteSize;
  cancelRequested: boolean;
}) {
  const { t } = useTranslation();
  const downloadedBytes = progress?.bytesDownloaded ?? 0;
  const totalBytes = progress?.bytesTotal ?? remoteSize?.totalBytes ?? 0;
  const percent =
    totalBytes > 0 ? Math.max(0, Math.min(100, (downloadedBytes / totalBytes) * 100)) : undefined;
  const failed = progress?.phase === 'failed';
  return (
    <div className="ol-model-download-progress">
      <header>
        <span>{t('localAsr.downloading')}</span>
        <span className={failed ? 'is-error' : undefined}>
          {failed
            ? t('localAsr.failed')
            : percent == null
              ? t('common.loading')
              : `${Math.round(percent)}%`}
        </span>
      </header>
      <progress
        className="ol-model-progress"
        max={100}
        value={percent}
        aria-label={t('localAsr.downloading')}
      />
      <span>
        {formatBytes(downloadedBytes)} /{' '}
        {totalBytes > 0 ? formatBytes(totalBytes) : t('localAsr.sizeUnknown')}
      </span>
      {progress?.file && <small>{progress.file}</small>}
      {failed && progress?.error && (
        <span className="is-error" role="alert">
          {progress.error}
        </span>
      )}
      {cancelRequested && <span role="status">{t('localAsr.foundryCancelRequested')}</span>}
    </div>
  );
}

export interface ModelRowProps {
  model: LocalAsrModelStatus;
  modelDir: string;
  remoteSize?: RemoteSize;
  progress?: LocalAsrDownloadProgress;
  isActive: boolean;
  engineAvailable: boolean;
  disabled: boolean;
  testing: boolean;
  testResult?: LocalAsrTestResult | { error: string };
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
  onReveal: () => void;
  onSetActive: () => void;
  onTest: () => void;
}

export function ModelRow({
  model,
  modelDir,
  remoteSize,
  progress,
  isActive,
  engineAvailable,
  disabled,
  testing,
  testResult,
  onDownload,
  onCancel,
  onDelete,
  onReveal,
  onSetActive,
  onTest,
}: ModelRowProps) {
  const { t } = useTranslation();
  const isDownloading = useMemo(
    () => progress?.phase === 'started' || progress?.phase === 'progress',
    [progress?.phase],
  );
  const downloadedBytes = progress?.bytesDownloaded ?? model.downloadedBytes;
  const totalBytes = progress?.bytesTotal ?? remoteSize?.totalBytes ?? 0;
  const ratio = totalBytes > 0 ? Math.min(1, downloadedBytes / totalBytes) : 0;
  // 进度条要保留：有 partial 残留（downloadedBytes>0 但未完整）就一直显示，
  // 让用户看到上次下到哪里了，再点下载会从那里续。
  const hasPartial = !model.isDownloaded && model.downloadedBytes > 0;
  const showProgress = isDownloading || progress?.phase === 'failed' || hasPartial;

  const sizeLabel = remoteSize?.loading
    ? t('localAsr.sizeLoading')
    : remoteSize?.error
      ? t('localAsr.sizeUnknown')
      : remoteSize && remoteSize.totalBytes > 0
        ? `${formatBytes(remoteSize.totalBytes)} · ${remoteSize.fileCount} ${t('localAsr.files')}`
        : t('localAsr.sizeUnknown');

  return (
    <Card>
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 16,
        }}
      >
        <div style={{ minWidth: 0 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              marginBottom: 4,
            }}
          >
            <div
              style={{
                fontSize: 14,
                fontWeight: 600,
                color: 'var(--ol-ink)',
              }}
            >
              {model.id}
            </div>
            {isActive && (
              <Pill tone="blue" size="sm">
                {t('localAsr.activeBadge')}
              </Pill>
            )}
            {model.isDownloaded && (
              <Pill tone="ok" size="sm">
                {t('localAsr.downloadedBadge')}
              </Pill>
            )}
          </div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            {model.hfRepo} · {sizeLabel}
          </div>
          <div
            style={{
              fontSize: 11,
              color: 'var(--ol-ink-4)',
              marginTop: 4,
              wordBreak: 'break-all',
            }}
          >
            {t('localAsr.modelDir')}: <code>{modelDir || '—'}</code>
          </div>
          {showProgress && (
            <div style={{ marginTop: 10, maxWidth: 420 }}>
              <div
                style={{
                  height: 6,
                  borderRadius: 3,
                  background: 'rgba(0,0,0,0.06)',
                  overflow: 'hidden',
                }}
              >
                <div
                  style={{
                    width: `${ratio * 100}%`,
                    height: '100%',
                    background:
                      progress?.phase === 'failed' ? '#d04545' : 'var(--ol-accent-blue, #2c5cff)',
                    transition: 'width 120ms linear',
                  }}
                />
              </div>
              <div
                style={{
                  fontSize: 11,
                  color: 'var(--ol-ink-4)',
                  marginTop: 6,
                }}
              >
                {progress?.phase === 'failed'
                  ? `${t('localAsr.failed')}: ${progress.error ?? ''}`
                  : `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` +
                    (progress?.file ? ` · ${progress.file}` : '')}
              </div>
            </div>
          )}
        </div>
        <div
          style={{
            display: 'flex',
            gap: 8,
            flexShrink: 0,
            flexWrap: 'wrap',
            justifyContent: 'flex-end',
            maxWidth: 360,
          }}
        >
          {model.isDownloaded ? (
            <>
              {!isActive && (
                <Btn
                  variant="blue"
                  size="sm"
                  disabled={disabled || !engineAvailable}
                  onClick={onSetActive}
                >
                  {t('localAsr.setActive')}
                </Btn>
              )}
              <Btn
                variant="primary"
                size="sm"
                disabled={disabled || testing || !engineAvailable}
                onClick={onTest}
              >
                {testing ? t('localAsr.testRunning') : t('localAsr.test')}
              </Btn>
              <Btn variant="ghost" size="sm" disabled={disabled || testing} onClick={onDelete}>
                {t('localAsr.delete')}
              </Btn>
              <Btn variant="ghost" size="sm" disabled={disabled} onClick={onReveal}>
                {t('localAsr.revealDir')}
              </Btn>
            </>
          ) : isDownloading ? (
            <Btn variant="ghost" size="sm" onClick={onCancel}>
              {t('localAsr.cancel')}
            </Btn>
          ) : (
            <>
              <Btn
                variant="primary"
                size="sm"
                disabled={disabled || !engineAvailable}
                onClick={onDownload}
              >
                {hasPartial ? t('localAsr.resume') : t('localAsr.download')}
              </Btn>
              {hasPartial && (
                <Btn variant="ghost" size="sm" disabled={disabled} onClick={onDelete}>
                  {t('localAsr.delete')}
                </Btn>
              )}
              <Btn variant="ghost" size="sm" disabled={disabled} onClick={onReveal}>
                {t('localAsr.revealDir')}
              </Btn>
            </>
          )}
        </div>
      </div>
      {testResult && <TestResultBlock result={testResult} />}
    </Card>
  );
}

export function TestResultBlock({ result }: { result: LocalAsrTestResult | { error: string } }) {
  const { t } = useTranslation();
  const hasError = 'error' in result;
  return (
    <div
      style={{
        marginTop: 12,
        padding: '10px 12px',
        background: 'var(--ol-surface-2)',
        borderRadius: 8,
        fontSize: 12.5,
        color: hasError ? 'var(--ol-err)' : 'var(--ol-ink-2)',
        lineHeight: 1.6,
      }}
    >
      {hasError ? (
        <div>
          <strong>{t('localAsr.testFailed')}: </strong>
          {result.error}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <div
            style={{
              fontSize: 11,
              color: 'var(--ol-ink-4)',
              letterSpacing: '.04em',
              textTransform: 'uppercase',
            }}
          >
            {t('localAsr.testHeading')}
          </div>
          <div>
            <span style={{ color: 'var(--ol-ink-4)' }}>{t('localAsr.testExpected')}: </span>
            {result.expectedText}
          </div>
          <div>
            <span style={{ color: 'var(--ol-ink-4)' }}>{t('localAsr.testActual')}: </span>
            <strong>{result.transcribedText || '(空)'}</strong>
          </div>
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>
            {t('localAsr.testStats', {
              audio: (result.audioMs / 1000).toFixed(1),
              load: (result.loadMs / 1000).toFixed(1),
              transcribe: (result.transcribeMs / 1000).toFixed(1),
              backend: result.backend,
            })}
          </div>
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────
// 本地 ASR 模型管理重构（两栏看板 + 下载弹框 + 右上角下载进度浮层）。
// 纯展示组件；数据与动作由 LocalAsr/index.tsx 组装后传入。
// ─────────────────────────────────────────────────────────────────────

/** 侧栏统一条目：本地引擎（Qwen3 / Whisper / sherpa-onnx / foundry）归一化。 */
export interface SidebarModelEntry {
  id: string;
  /** 展示名（如 qwen3-asr-0.6b / whisper-small）。 */
  name: string;
  /** HF 仓库标识（Qwen3 有；sherpa/foundry 可能为空）。 */
  repo?: string;
  /** 已下载字节数（HF 拉取的真实尺寸）。 */
  remoteBytes?: number;
  /** 已下载（有绿勾）。 */
  isDownloaded: boolean;
  /** 下载中（有进度条/取消入口）。 */
  isDownloading: boolean;
  /** 下载中实时百分比（0-100；仅 isDownloading 时有值）。 */
  percent?: number | null;
  /** 当前激活（设为默认的本地模型）。 */
  isActive: boolean;
  /** 引擎标识，决定右侧动作按钮分派。 */
  engine: 'qwen3' | 'whisper' | 'sherpa' | 'foundry';
  downloadError?: string | null;
}

const ENGINE_LABELS: Record<SidebarModelEntry['engine'], string> = {
  qwen3: 'Qwen3-ASR',
  whisper: 'Whisper',
  sherpa: 'sherpa-onnx',
  foundry: 'Foundry Local',
};

function ModelStatus({ entry }: { entry: SidebarModelEntry }) {
  const { t } = useTranslation();
  const label = entry.isDownloading
    ? t('localAsr.downloading')
    : entry.downloadError
      ? t('localAsr.failed')
      : entry.isActive
        ? t('localAsr.activePill')
        : entry.isDownloaded
          ? t('localAsr.downloadedBadge')
          : t('localAsr.notDownloadedBadge');
  return (
    <span
      className={`ol-model-status${entry.isActive ? ' is-active' : ''}${entry.downloadError ? ' is-error' : ''}`}
    >
      {entry.isDownloaded && !entry.isDownloading && <Icon name="check" size={13} />}
      {label}
    </span>
  );
}

/** Names get their own line; engine, size and state remain readable in every language. */
function ModelChoice({
  entry,
  selected,
  onSelect,
}: {
  entry: SidebarModelEntry;
  selected: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  return (
    <button type="button" className="ol-model-choice" aria-pressed={selected} onClick={onSelect}>
      <span className="ol-model-choice-name">{entry.name}</span>
      <span className="ol-model-choice-meta">
        <span>{ENGINE_LABELS[entry.engine]}</span>
        <span>
          {entry.remoteBytes ? formatBytes(entry.remoteBytes) : t('localAsr.sizeUnknown')}
        </span>
      </span>
      <span className="ol-model-choice-state">
        <ModelStatus entry={entry} />
        {entry.isDownloading && entry.percent != null && (
          <span>{Math.round(Math.max(0, Math.min(100, entry.percent)))}%</span>
        )}
      </span>
      {entry.isDownloading && (
        <progress
          className="ol-model-progress"
          max={100}
          value={entry.percent == null ? undefined : Math.max(0, Math.min(100, entry.percent))}
          aria-label={t('localAsr.downloading')}
        />
      )}
    </button>
  );
}

export function ModelSidebar({
  entries,
  selectedId,
  onSelect,
}: {
  entries: SidebarModelEntry[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="ol-model-library">
      <div className="ol-model-list-heading">
        <span>{t('localAsr.sidebarTitle')}</span>
        <span>{entries.length}</span>
      </div>
      <div className="ol-model-list" aria-label={t('localAsr.sidebarTitle')}>
        {entries.map((entry) => (
          <ModelChoice
            key={entry.id}
            entry={entry}
            selected={entry.id === selectedId}
            onSelect={() => onSelect(entry.id)}
          />
        ))}
      </div>
    </div>
  );
}

function ModelFacts({
  entry,
  fileCount,
  mirrorLabel,
}: {
  entry: SidebarModelEntry;
  fileCount: number | null;
  mirrorLabel?: string;
}) {
  const { t } = useTranslation();
  return (
    <dl className="ol-model-facts">
      <div>
        <dt>{t('localAsr.engineLabel')}</dt>
        <dd>{ENGINE_LABELS[entry.engine]}</dd>
      </div>
      <div>
        <dt>{t('localAsr.sizeLabel')}</dt>
        <dd>{entry.remoteBytes ? formatBytes(entry.remoteBytes) : t('localAsr.sizeUnknown')}</dd>
      </div>
      {fileCount != null && fileCount > 0 && (
        <div>
          <dt>{t('localAsr.files')}</dt>
          <dd>{fileCount}</dd>
        </div>
      )}
      {mirrorLabel && (
        <div>
          <dt>{t('localAsr.mirrorLabel')}</dt>
          <dd>{mirrorLabel}</dd>
        </div>
      )}
      {entry.repo && (
        <div className="ol-model-fact-wide">
          <dt>{t('localAsr.detailRepo')}</dt>
          <dd>{entry.repo}</dd>
        </div>
      )}
    </dl>
  );
}

/** Installed details never infer download activity from a terminal progress event. */
export function ModelDetailPanel({
  entry,
  fileCount,
  mirrorLabel,
  progress,
  busy,
  onDownload,
  onCancel,
  onDelete,
  onReveal,
  onTest,
  showTest,
  testResult,
  testing,
}: {
  entry: SidebarModelEntry | null;
  fileCount: number | null;
  mirrorLabel?: string;
  progress?: LocalAsrDownloadProgress;
  busy: boolean;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
  onReveal: () => void;
  onTest: () => void;
  showTest: boolean;
  testResult: LocalAsrTestResult | { error: string } | null;
  testing: boolean;
}) {
  const { t } = useTranslation();
  if (!entry) return <p className="ol-model-muted">{t('localAsr.detailEmpty')}</p>;
  return (
    <div className="ol-model-detail">
      <div className="ol-model-detail-heading">
        <span className="ol-model-eyebrow">{t('localAsr.detailsTitle')}</span>
        <h3>{entry.name}</h3>
        <ModelStatus entry={entry} />
      </div>
      <ModelFacts entry={entry} fileCount={fileCount} mirrorLabel={mirrorLabel} />
      {entry.isDownloading && <DownloadProgressBlock progress={progress} cancelRequested={false} />}
      {entry.downloadError && (
        <div className="ol-model-error" role="alert">
          {entry.downloadError}
        </div>
      )}
      {entry.isDownloaded && showTest && (
        <p className="ol-model-muted">{t('localAsr.testActivateHint')}</p>
      )}
      {testResult && <TestResultBlock result={testResult} />}
      <div className="ol-model-actions">
        {!entry.isDownloaded && !entry.isDownloading && (
          <Btn variant="blue" disabled={busy} onClick={onDownload}>
            {entry.downloadError ? t('common.retry') : t('localAsr.download')}
          </Btn>
        )}
        {entry.isDownloading && (
          <Btn variant="ghost" onClick={onCancel}>
            {t('localAsr.cancel')}
          </Btn>
        )}
        {entry.isDownloaded && showTest && (
          <Btn variant="blue" disabled={busy || testing} onClick={onTest}>
            {testing ? t('localAsr.testRunning') : t('localAsr.test')}
          </Btn>
        )}
        {entry.isDownloaded && (
          <>
            <Btn
              variant="ghost"
              disabled={busy || testing || entry.isDownloading}
              onClick={onReveal}
            >
              {t('localAsr.revealDir')}
            </Btn>
            <Btn
              variant="ghost"
              disabled={busy || testing || entry.isDownloading}
              onClick={onDelete}
              style={{ color: 'var(--ol-err)' }}
            >
              {t('localAsr.delete')}
            </Btn>
          </>
        )}
      </div>
    </div>
  );
}

/** Portal escapes the transformed window shell. The catalog and details scroll independently. */
export function DownloadDialog({
  entries,
  selectedId,
  onSelect,
  sizeOf,
  fileCountOf,
  hfCardOf,
  busy,
  loading,
  error,
  onRetryCard,
  onRetryCatalog,
  onStart,
  onClose,
}: {
  entries: SidebarModelEntry[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  sizeOf: (id: string) => number | null;
  fileCountOf: (id: string) => number | null;
  hfCardOf: (
    id: string,
  ) =>
    | { status: 'loading' }
    | { status: 'error'; message: string }
    | { status: 'ok'; card: HfModelCard }
    | null;
  busy: boolean;
  loading: boolean;
  error: string | null;
  onRetryCatalog: () => void;
  onRetryCard: (id: string) => void;
  onStart: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const stackLayout = useLayoutStack(760);
  const [showDetails, setShowDetails] = useState(false);
  const [engine, setEngine] = useState<SidebarModelEntry['engine'] | 'all'>('all');
  const titleId = useId();
  const descriptionId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const detailRef = useRef<HTMLHeadingElement>(null);
  const lastChoiceRef = useRef<string | null>(null);
  const engines = [...new Set(entries.map((entry) => entry.engine))];
  const visibleEntries =
    engine === 'all' ? entries : entries.filter((entry) => entry.engine === engine);
  const selected =
    visibleEntries.find((entry) => entry.id === selectedId) ??
    visibleEntries.find((entry) => !entry.isDownloaded) ??
    visibleEntries[0] ??
    null;
  const hfCard = selected ? hfCardOf(selected.id) : null;

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus({ preventScroll: true });
    return () => {
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);

  useEffect(() => {
    if (stackLayout && showDetails) detailRef.current?.focus({ preventScroll: true });
  }, [stackLayout, showDetails]);

  const selectModel = (id: string) => {
    lastChoiceRef.current = id;
    onSelect(id);
    setShowDetails(true);
  };

  const showCatalog = () => {
    setShowDetails(false);
    window.requestAnimationFrame(() => {
      const buttons = dialogRef.current?.querySelectorAll<HTMLButtonElement>('.ol-model-choice');
      const index = visibleEntries.findIndex((entry) => entry.id === lastChoiceRef.current);
      buttons?.[Math.max(0, index)]?.focus({ preventScroll: true });
    });
  };

  return createPortal(
    <div
      className="ol-model-dialog-overlay"
      onClick={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div
        ref={dialogRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={descriptionId}
        className={`ol-model-dialog${stackLayout ? ' is-stacked' : ''}`}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onClose();
          }
          if (event.key !== 'Tab') return;
          const focusable = Array.from(
            event.currentTarget.querySelectorAll<HTMLElement>(
              'button:not(:disabled), [href], input, select, [tabindex="0"]',
            ),
          ).filter((element) => element.getClientRects().length > 0);
          const first = focusable[0];
          const last = focusable[focusable.length - 1];
          const active = document.activeElement;
          if (event.shiftKey && (active === first || !focusable.includes(active as HTMLElement))) {
            event.preventDefault();
            last?.focus();
          } else if (
            !event.shiftKey &&
            (active === last || !focusable.includes(active as HTMLElement))
          ) {
            event.preventDefault();
            first?.focus();
          }
        }}
      >
        <header className="ol-model-dialog-header">
          <div>
            <h2 id={titleId}>{t('localAsr.downloadDialogTitle')}</h2>
            <p id={descriptionId}>{t('localAsr.downloadDialogDesc')}</p>
          </div>
          <button
            type="button"
            className="ol-model-close"
            aria-label={t('common.close')}
            onClick={onClose}
          >
            <Icon name="close" size={18} />
          </button>
        </header>
        <div className={`ol-model-dialog-body${entries.length === 0 ? ' is-empty' : ''}`}>
          {(!stackLayout || !showDetails || !selected) && (
            <div className="ol-model-catalog">
              <div className="ol-model-catalog-toolbar">
                <div className="ol-model-list-heading">
                  <span>{t('localAsr.catalogTitle')}</span>
                  <span>{visibleEntries.length}</span>
                </div>
                {engines.length > 1 && (
                  <div
                    className="ol-model-filters"
                    role="group"
                    aria-label={t('localAsr.engineLabel')}
                  >
                    {(['all', ...engines] as const).map((value) => (
                      <button
                        key={value}
                        type="button"
                        aria-pressed={engine === value}
                        onClick={() => {
                          setEngine(value);
                          const nextEntries =
                            value === 'all'
                              ? entries
                              : entries.filter((entry) => entry.engine === value);
                          if (!nextEntries.some((entry) => entry.id === selectedId)) {
                            const next =
                              nextEntries.find((entry) => !entry.isDownloaded) ?? nextEntries[0];
                            if (next) onSelect(next.id);
                          }
                        }}
                      >
                        {value === 'all' ? t('localAsr.allEngines') : ENGINE_LABELS[value]}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <div className="ol-model-list ol-model-catalog-list">
                {visibleEntries.map((entry) => (
                  <ModelChoice
                    key={entry.id}
                    entry={entry}
                    selected={entry.id === selected?.id}
                    onSelect={() => selectModel(entry.id)}
                  />
                ))}
                {visibleEntries.length === 0 && (
                  <div className="ol-model-catalog-empty">
                    <Icon name="download" size={28} />
                    <p className="ol-model-muted" role="status">
                      {loading ? t('common.loading') : error || t('localAsr.catalogEmpty')}
                    </p>
                    <Btn disabled={loading} onClick={onRetryCatalog}>
                      {t('localAsr.reloadCatalog')}
                    </Btn>
                  </div>
                )}
              </div>
            </div>
          )}
          {selected && (!stackLayout || showDetails) && (
            <section className="ol-model-catalog-detail">
              <div className="ol-model-catalog-content">
                {stackLayout && (
                  <button type="button" className="ol-model-back" onClick={showCatalog}>
                    ← {t('localAsr.backToCatalog')}
                  </button>
                )}
                {selected ? (
                  <>
                    <div className="ol-model-detail-heading">
                      <span className="ol-model-eyebrow">{ENGINE_LABELS[selected.engine]}</span>
                      <h3 ref={detailRef} tabIndex={-1}>
                        {selected.name}
                      </h3>
                      <ModelStatus entry={selected} />
                    </div>
                    <ModelFacts
                      entry={{ ...selected, remoteBytes: sizeOf(selected.id) ?? undefined }}
                      fileCount={fileCountOf(selected.id)}
                    />
                    {selected.downloadError && (
                      <div className="ol-model-error" role="alert">
                        {selected.downloadError}
                      </div>
                    )}
                    {hfCard?.status === 'loading' && (
                      <p className="ol-model-muted" role="status">
                        {t('common.loading')}
                      </p>
                    )}
                    {hfCard?.status === 'error' && (
                      <div className="ol-model-error">
                        <p>{t('localAsr.hfCardFailed')}</p>
                        <details>
                          <summary>{t('localAsr.errorDetails')}</summary>
                          <p>{hfCard.message}</p>
                        </details>
                        <Btn size="sm" onClick={() => onRetryCard(selected.id)}>
                          {t('common.retry')}
                        </Btn>
                      </div>
                    )}
                    {hfCard?.status === 'ok' && (
                      <div className="ol-model-description">
                        <h4>{t('localAsr.hfDescription')}</h4>
                        <p>{hfCard.card.description || t('localAsr.hfNoDescription')}</p>
                        <dl className="ol-model-facts">
                          <div>
                            <dt>{t('localAsr.hfDownloads')}</dt>
                            <dd>{hfCard.card.downloads.toLocaleString()}</dd>
                          </div>
                          <div>
                            <dt>{t('localAsr.hfLikes')}</dt>
                            <dd>{hfCard.card.likes.toLocaleString()}</dd>
                          </div>
                        </dl>
                      </div>
                    )}
                    {selected.isDownloaded && (
                      <p className="ol-model-muted">{t('localAsr.downloadDialogAlreadyHave')}</p>
                    )}
                  </>
                ) : (
                  <p className="ol-model-muted">{t('localAsr.detailEmpty')}</p>
                )}
              </div>
              <footer className="ol-model-dialog-footer">
                <span className="ol-model-muted">
                  {selected?.isDownloading
                    ? t('localAsr.downloading')
                    : t('localAsr.downloadProgressHint')}
                </span>
                <div className="ol-model-actions">
                  <Btn variant="ghost" onClick={onClose}>
                    {t('common.close')}
                  </Btn>
                  <Btn
                    variant="blue"
                    disabled={busy || !selected || selected.isDownloaded || selected.isDownloading}
                    onClick={onStart}
                  >
                    {selected?.isDownloaded
                      ? t('localAsr.downloadedBadge')
                      : selected?.downloadError
                        ? t('common.retry')
                        : t('localAsr.startDownload')}
                  </Btn>
                </div>
              </footer>
            </section>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
