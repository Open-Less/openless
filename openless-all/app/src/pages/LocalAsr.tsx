// LocalAsr.tsx — 本地 Qwen3-ASR 模型管理页。
//
// 功能：
//  - 顶部：当前激活模型 + 镜像源切换
//  - 模型列表：每行模型 = 真实尺寸 / 进度 / [下载|取消|删除|设为默认]
//  - 真实尺寸通过 fetchLocalAsrRemoteInfo 实时从 HuggingFace API 拉，**不硬编码**
//  - 监听 `local-asr-download-progress` 事件实时刷新进度
//  - Win 端引擎不可用时禁用下载按钮，提示见 issue #256

import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauri, setActiveAsrProvider } from '../lib/ipc';
import {
  cancelLocalAsrDownload,
  deleteLocalAsrModel,
  downloadLocalAsrModel,
  fetchLocalAsrRemoteInfo,
  getLocalAsrEngineStatus,
  getLocalAsrSettings,
  listLocalAsrModels,
  preloadLocalAsr,
  releaseLocalAsrEngine,
  setLocalAsrActiveModel,
  setLocalAsrKeepLoadedSecs,
  setLocalAsrMirror,
  testLocalAsrModel,
  type LocalAsrDownloadProgress,
  type LocalAsrEngineStatus,
  type LocalAsrModelStatus,
  type LocalAsrSettings,
  type LocalAsrTestResult,
} from '../lib/localAsr';
import { Btn, Card, PageHeader, Pill } from './_atoms';

interface RemoteSize {
  totalBytes: number;
  fileCount: number;
  loading: boolean;
  error: string | null;
}

export function LocalAsr() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<LocalAsrSettings | null>(null);
  const [models, setModels] = useState<LocalAsrModelStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, LocalAsrDownloadProgress>>({});
  const [remoteSizes, setRemoteSizes] = useState<Record<string, RemoteSize>>({});
  const [error, setError] = useState<string | null>(null);
  const [busyModelId, setBusyModelId] = useState<string | null>(null);
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, LocalAsrTestResult | { error: string }>>({});
  const [engineStatus, setEngineStatus] = useState<LocalAsrEngineStatus | null>(null);
  const refreshTimer = useRef<number | null>(null);
  const engineStatusTimer = useRef<number | null>(null);

  const refreshEngineStatus = async () => {
    try {
      const status = await getLocalAsrEngineStatus();
      setEngineStatus(status);
    } catch (err) {
      console.warn('[localAsr] engine status query failed', err);
    }
  };

  const refresh = async () => {
    try {
      setError(null);
      const [s, list] = await Promise.all([getLocalAsrSettings(), listLocalAsrModels()]);
      setSettings(s);
      setModels(list);
      void refreshEngineStatus();
      // 拉远端真实尺寸（每个模型一次，结果留缓存）
      void Promise.all(
        list.map(async m => {
          await ensureRemoteSize(m.id, s.mirror);
        }),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const ensureRemoteSize = async (modelId: string, mirror: string) => {
    setRemoteSizes(prev => {
      if (prev[modelId] && !prev[modelId].error) return prev;
      return { ...prev, [modelId]: { totalBytes: 0, fileCount: 0, loading: true, error: null } };
    });
    try {
      const info = await fetchLocalAsrRemoteInfo(modelId, mirror);
      setRemoteSizes(prev => ({
        ...prev,
        [modelId]: {
          totalBytes: info.totalBytes,
          fileCount: info.files.length,
          loading: false,
          error: null,
        },
      }));
    } catch (e) {
      setRemoteSizes(prev => ({
        ...prev,
        [modelId]: {
          totalBytes: 0,
          fileCount: 0,
          loading: false,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    }
  };

  useEffect(() => {
    void refresh();
    // 引擎状态每 5s 轮询一次，让 UI 能看到 release 计时器到点后的状态变化
    engineStatusTimer.current = window.setInterval(() => {
      void refreshEngineStatus();
    }, 5000);
    return () => {
      if (engineStatusTimer.current !== null) {
        window.clearInterval(engineStatusTimer.current);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 镜像变更后重拉一次远端尺寸（不同镜像 API 返回的 size 数值是一致的，
  // 但请求路径不同——切镜像时强制刷新一次让用户看到新源能否访通）。
  useEffect(() => {
    if (!settings) return;
    setRemoteSizes({});
    void Promise.all(
      models.map(m => ensureRemoteSize(m.id, settings.mirror)),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings?.mirror]);

  // 订阅下载进度事件 — 仅 Tauri 环境（浏览器 dev mock 无事件）。
  useEffect(() => {
    if (!isTauri) return;
    let unlisten: undefined | (() => void);
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const off = await listen<LocalAsrDownloadProgress>('local-asr-download-progress', e => {
        const payload = e.payload;
        if (payload.phase === 'cancelled') {
          // 取消时清条目，bar 是否还显示交给 hasPartial 判断
          setProgress(prev => {
            const next = { ...prev };
            delete next[payload.modelId];
            return next;
          });
        } else {
          setProgress(prev => ({ ...prev, [payload.modelId]: payload }));
        }
        if (
          payload.phase === 'finished' ||
          payload.phase === 'cancelled' ||
          payload.phase === 'failed'
        ) {
          if (refreshTimer.current) window.clearTimeout(refreshTimer.current);
          refreshTimer.current = window.setTimeout(() => {
            void refresh();
          }, 200);
        }
      });
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    })().catch(err => console.warn('[localAsr] subscribe failed', err));
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
      if (refreshTimer.current) window.clearTimeout(refreshTimer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSetActiveModel = async (modelId: string) => {
    setBusyModelId(modelId);
    try {
      await setLocalAsrActiveModel(modelId);
      // 顺手把 active provider 也切到本地（避免用户改了模型却忘了切 provider）
      await setActiveAsrProvider('local-qwen3');
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyModelId(null);
    }
  };

  const handleDownload = async (modelId: string) => {
    setBusyModelId(modelId);
    // 重下载时，第一个后端事件到达前先用本地已知值占位，避免进度条从 0% 跳到真实位置。
    // 优先级：上一次 progress（取消后已删，通常没有）→ models 里的 downloadedBytes（cancel 时乐观写入）
    const model = models.find(m => m.id === modelId);
    const initialDownloaded =
      progress[modelId]?.bytesDownloaded ?? model?.downloadedBytes ?? 0;
    setProgress(prev => ({
      ...prev,
      [modelId]: {
        modelId,
        file: '',
        fileIndex: 0,
        fileCount: remoteSizes[modelId]?.fileCount ?? 0,
        bytesDownloaded: initialDownloaded,
        bytesTotal: remoteSizes[modelId]?.totalBytes ?? 0,
        phase: 'started',
        error: null,
      },
    }));
    try {
      await downloadLocalAsrModel(modelId, settings?.mirror);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setProgress(prev => {
        const cur = prev[modelId];
        if (cur?.phase === 'started') {
          return { ...prev, [modelId]: { ...cur, phase: 'failed', error: e instanceof Error ? e.message : String(e) } };
        }
        return prev;
      });
    } finally {
      setBusyModelId(null);
    }
  };

  const handleCancel = async (modelId: string) => {
    // Progress 事件里的 bytesDownloaded 是后端 in_flight + already_done，是真实字节
    const lastBytes = progress[modelId]?.bytesDownloaded ?? 0;
    try {
      await cancelLocalAsrDownload(modelId);
      setProgress(prev => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      // 乐观更新：让 hasPartial 立刻翻 true，不等 listener 200ms 后的 refresh
      if (lastBytes > 0) {
        setModels(prev =>
          prev.map(m => (m.id === modelId ? { ...m, downloadedBytes: lastBytes } : m)),
        );
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleDelete = async (modelId: string) => {
    setBusyModelId(modelId);
    try {
      await deleteLocalAsrModel(modelId);
      setProgress(prev => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyModelId(null);
    }
  };

  const handleKeepLoadedChange = async (seconds: number) => {
    try {
      await setLocalAsrKeepLoadedSecs(seconds);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleReleaseEngine = async () => {
    try {
      await releaseLocalAsrEngine();
      await refreshEngineStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handlePreload = async () => {
    try {
      await preloadLocalAsr();
      // 触发预加载后给后端几秒，再查状态
      window.setTimeout(() => void refreshEngineStatus(), 1500);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleTest = async (modelId: string) => {
    setTestingModelId(modelId);
    setTestResults(prev => {
      const next = { ...prev };
      delete next[modelId];
      return next;
    });
    try {
      const result = await testLocalAsrModel(modelId);
      setTestResults(prev => ({ ...prev, [modelId]: result }));
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setTestResults(prev => ({ ...prev, [modelId]: { error: message } }));
    } finally {
      setTestingModelId(null);
    }
  };

  const handleMirrorChange = async (mirror: string) => {
    try {
      await setLocalAsrMirror(mirror);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const engineAvailable = settings?.engineAvailable ?? false;

  return (
    <div style={{ padding: '20px 28px 32px', overflowY: 'auto', height: '100%' }}>
      <PageHeader
        kicker={t('localAsr.kicker')}
        title={t('localAsr.title')}
        desc={t('localAsr.desc')}
      />

      {/* 性能/质量预期警告 —— 用户硬要求要写清楚 */}
      <Card style={{ marginBottom: 16, background: 'rgba(255, 215, 130, 0.18)' }}>
        <div style={{ fontSize: 13, color: 'var(--ol-ink-2)', lineHeight: 1.6 }}>
          ⚠️ {t('localAsr.performanceWarning')}
        </div>
      </Card>

      {!engineAvailable && (
        <Card style={{ marginBottom: 16, background: 'rgba(255, 235, 200, 0.4)' }}>
          <div style={{ fontSize: 13, color: 'var(--ol-ink-2)' }}>
            {t('localAsr.engineUnavailable')}
          </div>
        </Card>
      )}

      <Card style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}>
          <div>
            <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-4)', marginBottom: 4 }}>
              {t('localAsr.mirrorLabel')}
            </div>
            <div style={{ fontSize: 13, color: 'var(--ol-ink-3)' }}>
              {t('localAsr.mirrorDesc')}
            </div>
          </div>
          <select
            value={settings?.mirror ?? 'huggingface'}
            onChange={e => void handleMirrorChange(e.target.value)}
            style={{
              fontSize: 13,
              padding: '6px 10px',
              borderRadius: 8,
              border: '0.5px solid rgba(0,0,0,0.12)',
              background: 'var(--ol-surface)',
              color: 'var(--ol-ink)',
              minWidth: 200,
            }}>
            <option value="huggingface">{t('localAsr.mirrorHuggingface')}</option>
            <option value="hf-mirror">{t('localAsr.mirrorHfMirror')}</option>
          </select>
        </div>
      </Card>

      {/* 运行时设置卡：内存中的引擎状态 + 多久释放 + 立即释放 */}
      {engineAvailable && (
        <Card style={{ marginBottom: 16 }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
              <div>
                <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-4)', marginBottom: 4 }}>
                  {t('localAsr.engineStatusLabel')}
                </div>
                <div style={{ fontSize: 13, color: 'var(--ol-ink-3)' }}>
                  {engineStatus?.loaded
                    ? t('localAsr.engineLoaded', { model: engineStatus.modelId ?? '' })
                    : t('localAsr.engineUnloaded')}
                </div>
              </div>
              <div style={{ display: 'flex', gap: 8 }}>
                {engineStatus?.loaded ? (
                  <Btn variant="ghost" size="sm" onClick={() => void handleReleaseEngine()}>
                    {t('localAsr.releaseNow')}
                  </Btn>
                ) : (
                  <Btn variant="ghost" size="sm" onClick={() => void handlePreload()}>
                    {t('localAsr.loadNow')}
                  </Btn>
                )}
              </div>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12, flexWrap: 'wrap' }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-4)', marginBottom: 4 }}>
                  {t('localAsr.keepLoadedLabel')}
                </div>
                <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.5 }}>
                  {t('localAsr.keepLoadedDesc')}
                </div>
              </div>
              <select
                value={engineStatus?.keepLoadedSecs ?? 300}
                onChange={e => void handleKeepLoadedChange(Number(e.target.value))}
                style={{
                  fontSize: 13,
                  padding: '6px 10px',
                  borderRadius: 8,
                  border: '0.5px solid rgba(0,0,0,0.12)',
                  background: 'var(--ol-surface)',
                  color: 'var(--ol-ink)',
                  minWidth: 200,
                }}>
                <option value={0}>{t('localAsr.keepImmediate')}</option>
                <option value={60}>{t('localAsr.keep1min')}</option>
                <option value={300}>{t('localAsr.keep5min')}</option>
                <option value={1800}>{t('localAsr.keep30min')}</option>
                <option value={86400}>{t('localAsr.keepForever')}</option>
              </select>
            </div>
          </div>
        </Card>
      )}

      {error && (
        <Card style={{ marginBottom: 16, background: 'rgba(255, 220, 220, 0.5)' }}>
          <div style={{ fontSize: 13, color: '#9b2c2c' }}>{error}</div>
        </Card>
      )}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {models.map(model => (
          <ModelRow
            key={model.id}
            model={model}
            remoteSize={remoteSizes[model.id]}
            progress={progress[model.id]}
            isActive={settings?.activeModel === model.id}
            engineAvailable={engineAvailable}
            disabled={busyModelId !== null && busyModelId !== model.id}
            testing={testingModelId === model.id}
            testResult={testResults[model.id]}
            onDownload={() => void handleDownload(model.id)}
            onCancel={() => void handleCancel(model.id)}
            onDelete={() => void handleDelete(model.id)}
            onSetActive={() => void handleSetActiveModel(model.id)}
            onTest={() => void handleTest(model.id)}
          />
        ))}
      </div>
    </div>
  );
}

interface ModelRowProps {
  model: LocalAsrModelStatus;
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
  onSetActive: () => void;
  onTest: () => void;
}

function ModelRow({
  model,
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
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}>
        <div style={{ minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
            <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{model.id}</div>
            {isActive && <Pill tone="blue" size="sm">{t('localAsr.activeBadge')}</Pill>}
            {model.isDownloaded && <Pill tone="ok" size="sm">{t('localAsr.downloadedBadge')}</Pill>}
          </div>
          <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>
            {model.hfRepo} · {sizeLabel}
          </div>
          {showProgress && (
            <div style={{ marginTop: 10, maxWidth: 420 }}>
              <div
                style={{
                  height: 6,
                  borderRadius: 3,
                  background: 'rgba(0,0,0,0.06)',
                  overflow: 'hidden',
                }}>
                <div
                  style={{
                    width: `${ratio * 100}%`,
                    height: '100%',
                    background:
                      progress?.phase === 'failed'
                        ? '#d04545'
                        : 'var(--ol-accent-blue, #2c5cff)',
                    transition: 'width 120ms linear',
                  }}
                />
              </div>
              <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 6 }}>
                {progress?.phase === 'failed'
                  ? `${t('localAsr.failed')}: ${progress.error ?? ''}`
                  : `${formatBytes(downloadedBytes)} / ${formatBytes(totalBytes)}` +
                    (progress?.file ? ` · ${progress.file}` : '')}
              </div>
            </div>
          )}
        </div>
        <div style={{ display: 'flex', gap: 8, flexShrink: 0, flexWrap: 'wrap', justifyContent: 'flex-end', maxWidth: 360 }}>
          {model.isDownloaded ? (
            <>
              {!isActive && (
                <Btn
                  variant="blue"
                  size="sm"
                  disabled={disabled || !engineAvailable}
                  onClick={onSetActive}>
                  {t('localAsr.setActive')}
                </Btn>
              )}
              <Btn
                variant="primary"
                size="sm"
                disabled={disabled || testing || !engineAvailable}
                onClick={onTest}>
                {testing ? t('localAsr.testRunning') : t('localAsr.test')}
              </Btn>
              <Btn variant="ghost" size="sm" disabled={disabled || testing} onClick={onDelete}>
                {t('localAsr.delete')}
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
                onClick={onDownload}>
                {hasPartial ? t('localAsr.resume') : t('localAsr.download')}
              </Btn>
              {hasPartial && (
                <Btn variant="ghost" size="sm" disabled={disabled} onClick={onDelete}>
                  {t('localAsr.delete')}
                </Btn>
              )}
            </>
          )}
        </div>
      </div>
      {testResult && <TestResultBlock result={testResult} />}
    </Card>
  );
}

function TestResultBlock({ result }: { result: LocalAsrTestResult | { error: string } }) {
  const { t } = useTranslation();
  const hasError = 'error' in result;
  return (
    <div
      style={{
        marginTop: 12,
        padding: '10px 12px',
        background: hasError ? 'rgba(255, 220, 220, 0.5)' : 'rgba(0, 0, 0, 0.04)',
        borderRadius: 8,
        fontSize: 12.5,
        color: hasError ? '#9b2c2c' : 'var(--ol-ink-2)',
        lineHeight: 1.6,
      }}>
      {hasError ? (
        <div>
          <strong>{t('localAsr.testFailed')}: </strong>{result.error}
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
          <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', letterSpacing: '.04em', textTransform: 'uppercase' }}>
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

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(0)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
