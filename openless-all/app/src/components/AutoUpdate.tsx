// 自动更新共用模块 — Settings 的"关于"section 和 footer 按钮共用同一套
// 状态机 + 对话框 UI。两边各自调用 useAutoUpdate()，dialog 渲染条件相同。
//
// 渠道感知：check 不再走 plugin-updater 的 JS check()（它只看 tauri.conf 配的
// Stable manifest URL），改为 invoke('app_check_update_with_channel')。
// Rust 那边按 prefs.update_channel 决定 manifest URL；返回的 metadata 直接
// `new Update(metadata)` 复用 plugin 的 download / install / close 实现，
// 我们不重复造下载和签名校验。

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { DownloadEvent } from '@tauri-apps/plugin-updater';
import { Update } from '@tauri-apps/plugin-updater';
import { useTranslation } from 'react-i18next';
import { isTauri, restartApp } from '../lib/ipc';
import { Btn } from '../pages/_atoms';

const UPDATE_CHECK_TIMEOUT_MS = 8_000; // 缩短超时，让镜像站慢的情况能更快 fallback

interface AppUpdateMetadata {
  rid: number;
  currentVersion: string;
  version: string;
  date?: string | null;
  body?: string | null;
  rawJson: Record<string, unknown>;
}

export type UpdateStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'none'
  | 'downloading'
  | 'installing'
  | 'downloaded'
  | 'error';

export interface UseAutoUpdate {
  status: UpdateStatus;
  version: string;
  progress: number | null;
  downloaded: number;
  contentLength: number | null;
  checking: boolean;
  busy: boolean;
  /** 触发"检查更新"。如果发现新版本，状态变为 'available'，需要 caller 渲染对话框让用户确认下载。 */
  checkForUpdates: () => Promise<void>;
  /** 用户在对话框里确认 → 下载 + 安装。完成后状态变为 'downloaded'，等用户点重启。 */
  installUpdate: () => Promise<void>;
  /** 关闭对话框（仅在非 busy 状态可用）。 */
  dismissDialog: () => Promise<void>;
}

export function useAutoUpdate(): UseAutoUpdate {
  const updateRef = useRef<Update | null>(null);
  const [status, setStatus] = useState<UpdateStatus>('idle');
  const [version, setVersion] = useState('');
  const [downloaded, setDownloaded] = useState(0);
  const [contentLength, setContentLength] = useState<number | null>(null);

  const checking = status === 'checking';
  const busy = status === 'downloading' || status === 'installing';
  const progress = contentLength && contentLength > 0
    ? Math.min(100, Math.round((downloaded / contentLength) * 100))
    : null;

  const closeUpdate = async () => {
    const current = updateRef.current;
    updateRef.current = null;
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

  const resetProgress = () => {
    setDownloaded(0);
    setContentLength(null);
  };

  const checkForUpdates = async () => {
    setStatus('checking');
    setVersion('');
    resetProgress();
    await closeUpdate();
    try {
      if (!isTauri) {
        setStatus('none');
        return;
      }
      // Rust 侧按 update_channel 拼 manifest URL：Stable → tauri.conf 默认；
      // Beta → fetch_latest_beta_release 拼出 -beta manifest URL 后再 check。
      const metadata = await invoke<AppUpdateMetadata | null>('app_check_update_with_channel', {
        timeoutMs: UPDATE_CHECK_TIMEOUT_MS,
      });
      if (!metadata) {
        setStatus('none');
        return;
      }
      // metadata 形状跟 plugin 自己 check 返回的 UpdateMetadata 完全一致；
      // new Update(metadata) 直接复用 plugin 的 download/install/close 实现。
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
      setStatus('error');
    }
  };

  const installUpdate = async () => {
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
      await closeUpdate();
      setStatus('error');
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
    checkForUpdates,
    installUpdate,
    dismissDialog,
  };
}

export function isDialogStatus(status: UpdateStatus): status is 'available' | 'downloading' | 'installing' | 'downloaded' {
  return status === 'available' || status === 'downloading' || status === 'installing' || status === 'downloaded';
}

export function UpdateDialog({
  status,
  version,
  progress,
  downloaded,
  contentLength,
  onInstall,
  onClose,
}: {
  status: 'available' | 'downloading' | 'installing' | 'downloaded';
  version: string;
  progress: number | null;
  downloaded: number;
  contentLength: number | null;
  onInstall: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const downloading = status === 'downloading';
  const installing = status === 'installing';
  return (
    <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.18)', display: 'grid', placeItems: 'center', zIndex: 40 }}>
      <div style={{ width: 360, borderRadius: 16, background: 'var(--ol-surface)', border: '0.5px solid var(--ol-line-strong)', boxShadow: '0 18px 42px rgba(0,0,0,0.18)', padding: 18 }}>
        <div style={{ fontSize: 15, fontWeight: 650, marginBottom: 8 }}>{t(`settings.about.updateDialog.${status}.title`)}</div>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)', lineHeight: 1.6, marginBottom: 14 }}>
          {t(`settings.about.updateDialog.${status}.desc`, { version })}
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
          {status === 'downloaded' && <Btn variant="blue" size="sm" onClick={restartApp}>{t('settings.about.updateDialog.restartNow')}</Btn>}
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
