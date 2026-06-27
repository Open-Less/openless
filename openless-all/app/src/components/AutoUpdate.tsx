// 自动更新共用模块 — Settings 的"关于"section 和 footer 按钮共用同一套
// 状态机 + 对话框 UI。两边各自调用 useAutoUpdate()，dialog 渲染条件相同。
//
// 渠道感知：check 走 appCheckUpdateWithChannel()（Rust 按渠道拼 manifest URL）。
// 桌面：download/install 复用 plugin-updater 的 Update 类。
// Android：download/install 走 appDownloadAndInstallAndroidUpdate（minisign + 系统安装器）。

import { useEffect, useRef, useState } from 'react';
import type { DownloadEvent } from '@tauri-apps/plugin-updater';
import { Update } from '@tauri-apps/plugin-updater';
import { listen } from '@tauri-apps/api/event';
import { useTranslation } from 'react-i18next';
import {
  appCheckUpdateWithChannel,
  appDownloadAndInstallAndroidUpdate,
  isAndroid,
  isTauri,
  logClientError,
  openExternal,
  restartApp,
  type AppUpdateMetadata,
  type UpdateChannel,
} from '../lib/ipc';
import { Btn } from '../pages/_atoms';

const UPDATE_CHECK_TIMEOUT_MS = 15_000;

// 自动更新失败时的手动下载兜底：直达 GitHub Releases（与「关于」页 RELEASE_NOTES_URL 一致）。
const RELEASE_DOWNLOAD_URL = 'https://github.com/appergb/openless/releases';

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'none'
  | 'downloading'
  | 'installing'
  | 'downloaded'
  | 'error'
  // installError：下载/安装这一步失败（区别于 'error' 的「检查失败」）。
  // 检查失败沿用 'error'，在 CheckUpdateButton 里只做按钮内轻提示、不弹框，
  // 后台自动检查失败也不弹框；只有 installError 才让弹框留在原地显示错误 + 手动下载兜底。
  | 'installError';

export type CheckUpdateOptions = {
  /** Android only: skip confirmation dialog and download + open system installer. */
  autoInstallAndroid?: boolean;
};

export interface UseAutoUpdate {
  status: UpdateStatus;
  version: string;
  progress: number | null;
  downloaded: number;
  contentLength: number | null;
  checking: boolean;
  busy: boolean;
  errorMessage: string | null;
  checkForUpdates: (channel?: UpdateChannel, options?: CheckUpdateOptions) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissDialog: () => Promise<void>;
}

type AndroidUpdatePayload = {
  url: string;
  signature: string;
  version: string;
};

export function useAutoUpdate(): UseAutoUpdate {
  const updateRef = useRef<Update | null>(null);
  const androidUpdateRef = useRef<AndroidUpdatePayload | null>(null);
  const [status, setStatus] = useState<UpdateStatus>('idle');
  const [version, setVersion] = useState('');
  const [downloaded, setDownloaded] = useState(0);
  const [contentLength, setContentLength] = useState<number | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const checking = status === 'checking';
  const busy = status === 'downloading' || status === 'installing';
  const progress = contentLength && contentLength > 0
    ? Math.min(100, Math.round((downloaded / contentLength) * 100))
    : null;

  const closeUpdate = async () => {
    const current = updateRef.current;
    updateRef.current = null;
    androidUpdateRef.current = null;
    if (current) {
      try {
        await current.close();
      } catch (error) {
        console.warn('[updater] failed to close update resource', error);
      }
    }
  };

  useEffect(() => {
    return () => { void closeUpdate(); };
  }, []);

  useEffect(() => {
    if (!isTauri || !isAndroid()) return;
    let unlisten: (() => void) | undefined;
    void listen<{
      downloaded: number;
      contentLength: number | null;
      phase: string;
    }>('android-update:progress', (event) => {
      setDownloaded(event.payload.downloaded);
      setContentLength(event.payload.contentLength);
      if (event.payload.phase === 'installing') {
        setStatus('installing');
      } else {
        setStatus('downloading');
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const resetProgress = () => {
    setDownloaded(0);
    setContentLength(null);
  };

  const storeAndroidMetadata = (metadata: AppUpdateMetadata) => {
    const raw = metadata.rawJson ?? {};
    const url = typeof raw.url === 'string' ? raw.url : '';
    const signature = typeof raw.signature === 'string' ? raw.signature : '';
    if (!url || !signature) {
      throw new Error('更新清单缺少 url 或 signature');
    }
    androidUpdateRef.current = { url, signature, version: metadata.version };
  };

  const checkForUpdates = async (channel?: UpdateChannel, options?: CheckUpdateOptions) => {
    setStatus('checking');
    setVersion('');
    setErrorMessage(null);
    resetProgress();
    await closeUpdate();
    try {
      if (!isTauri) {
        setStatus('none');
        return;
      }
      const metadata = await appCheckUpdateWithChannel(
        UPDATE_CHECK_TIMEOUT_MS,
        channel ?? null,
      );
      if (!metadata) {
        setStatus('none');
        return;
      }
      if (isAndroid()) {
        storeAndroidMetadata(metadata);
        setVersion(metadata.version);
        if (options?.autoInstallAndroid) {
          try {
            // 复用 storeAndroidMetadata 的 rawJson 解析（提取 url/signature/version）
            const raw = metadata.rawJson ?? {};
            const url = typeof raw.url === 'string' ? raw.url : '';
            const signature = typeof raw.signature === 'string' ? raw.signature : '';
            if (!url || !signature) {
              console.warn('[auto-update] android manifest missing url/signature, falling back to manual update');
              setStatus('available');
              return;
            }
            await appDownloadAndInstallAndroidUpdate({ url, signature, version: metadata.version });
            setStatus('downloaded');
          } catch (error) {
            console.warn('[auto-update] android auto-install failed', error);
            setStatus('available');
          }
          return;
        }
        setStatus('available');
        return;
      }
      const next = new Update({
        rid: metadata.rid,
        currentVersion: metadata.currentVersion,
        version: metadata.version,
        date: metadata.date ?? undefined,
        body: metadata.body ?? undefined,
        rawJson: metadata.rawJson,
      });
      updateRef.current = next;
      setVersion(next.version);
      setStatus('available');
    } catch (error) {
      console.error('[updater] failed to check update', error);
      const msg = error instanceof Error ? error.message : String(error);
      void logClientError(`[updater] check failed: ${msg}`);
      setErrorMessage(msg);
      setStatus('error');
    }
  };

  const installUpdate = async () => {
    if (isAndroid()) {
      const payload = androidUpdateRef.current;
      if (!payload) return;
      resetProgress();
      setStatus('downloading');
      try {
        await appDownloadAndInstallAndroidUpdate(payload);
        androidUpdateRef.current = null;
        setStatus('downloaded');
      } catch (error) {
        console.error('[updater] failed to install android update', error);
        const msg = error instanceof Error ? error.message : String(error);
        void logClientError(`[updater] android install failed (v${payload.version}): ${msg}`);
        setErrorMessage(msg);
        setStatus('installError');
      }
      return;
    }

    const update = updateRef.current;
    if (!update) return;
    resetProgress();
    setStatus('downloading');
    try {
      await update.download((event: DownloadEvent) => {
        if (event.event === 'Started') {
          resetProgress();
          setContentLength(event.data.contentLength ?? null);
        } else if (event.event === 'Progress') {
          setDownloaded(value => value + event.data.chunkLength);
        } else if (event.event === 'Finished') {
          setStatus('installing');
        }
      });
      setStatus('installing');
      await update.install();
      await closeUpdate();
      setStatus('downloaded');
    } catch (error) {
      console.error('[updater] failed to install update', error);
      const msg = error instanceof Error ? error.message : String(error);
      void logClientError(`[updater] install failed (v${update.version}): ${msg}`);
      setErrorMessage(msg);
      await closeUpdate();
      setStatus('installError');
    }
  };

  const dismissDialog = async () => {
    if (busy) return;
    await closeUpdate();
    setStatus('idle');
    setVersion('');
    resetProgress();
  };

  return {
    status,
    version,
    progress,
    downloaded,
    contentLength,
    checking,
    busy,
    errorMessage,
    checkForUpdates,
    installUpdate,
    dismissDialog,
  };
}

export function isDialogStatus(status: UpdateStatus): status is 'available' | 'downloading' | 'installing' | 'downloaded' | 'installError' {
  return status === 'available' || status === 'downloading' || status === 'installing' || status === 'downloaded' || status === 'installError';
}

export function UpdateDialog({
  status,
  version,
  progress,
  downloaded,
  contentLength,
  errorMessage,
  onInstall,
  onClose,
}: {
  status: 'available' | 'downloading' | 'installing' | 'downloaded' | 'installError';
  version: string;
  progress: number | null;
  downloaded: number;
  contentLength: number | null;
  errorMessage?: string | null;
  onInstall: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const downloading = status === 'downloading';
  const installing = status === 'installing';
  const installError = status === 'installError';
  const androidInstalled = isAndroid() && status === 'downloaded';
  return (
    <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.18)', display: 'grid', placeItems: 'center', zIndex: 40 }}>
      <div style={{ width: 360, borderRadius: 16, background: 'var(--ol-surface)', border: '0.5px solid var(--ol-line-strong)', boxShadow: '0 18px 42px rgba(0,0,0,0.18)', padding: 18 }}>
        <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 8 }}>{t(`settings.about.updateDialog.${status}.title`)}</div>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6, marginBottom: 14, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
          {androidInstalled
            ? t('settings.about.updateDialog.androidInstalled.desc', { version, defaultValue: '系统安装器已打开，请按提示完成安装。安装后重新打开 OpenLess 即可使用 {{version}}。' })
            : installError
              ? t('settings.about.updateDialog.installError.desc', { error: errorMessage || t('settings.about.updateError') })
              : t(`settings.about.updateDialog.${status}.desc`, { version })}
        </div>
        {(downloading || installing || status === 'downloaded') && (
          <div style={{ marginBottom: 14 }}>
            <div style={{ height: 8, borderRadius: 999, background: 'var(--ol-surface-2)', overflow: 'hidden', border: '0.5px solid var(--ol-line)' }}>
              <div style={{ height: '100%', width: `${status === 'downloaded' || installing ? 100 : progress ?? 8}%`, background: 'var(--ol-blue)', transition: 'width 0.18s var(--ol-motion-soft)' }} />
            </div>
            <div style={{ marginTop: 6, fontSize: 11, color: 'var(--ol-ink-4)' }}>
              {installing
                ? t('settings.about.updateDialog.installingLabel')
                : progress === null
                  ? t('settings.about.updateDialog.progressUnknown', { downloaded: formatBytes(downloaded) })
                  : t('settings.about.updateDialog.progress', { progress, downloaded: formatBytes(downloaded), total: formatBytes(contentLength ?? 0) })}
            </div>
          </div>
        )}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          {status === 'available' && <Btn size="sm" onClick={onClose}>{t('common.cancel')}</Btn>}
          {status === 'available' && <Btn variant="blue" size="sm" onClick={onInstall}>{t('settings.about.updateDialog.install')}</Btn>}
          {(downloading || installing) && <Btn size="sm" disabled>{installing ? t('settings.about.updateDialog.installingLabel') : t('settings.about.updateDialog.downloadingLabel')}</Btn>}
          {status === 'downloaded' && <Btn size="sm" onClick={onClose}>{t('settings.about.updateDialog.later')}</Btn>}
          {status === 'downloaded' && !androidInstalled && <Btn variant="blue" size="sm" onClick={restartApp}>{t('settings.about.updateDialog.restartNow')}</Btn>}
          {installError && <Btn size="sm" onClick={onClose}>{t('common.cancel')}</Btn>}
          {installError && <Btn variant="blue" size="sm" onClick={() => void openExternal(RELEASE_DOWNLOAD_URL)}>{t('settings.about.updateDialog.manualDownload')}</Btn>}
        </div>
      </div>
    </div>
  );
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return '0 B';
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
