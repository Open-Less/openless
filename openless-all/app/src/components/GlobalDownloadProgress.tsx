// GlobalDownloadProgress.tsx —— 全局下载进度浮层。
//
// 主窗口任何页面常驻显示（portal 到 document.body，fixed 右上角）：下载开始即
// 出现、结束前一直显示，切页面 / 开关设置弹窗都不影响它。组件自包含——自己
// 订阅三套本地 ASR 引擎的下载进度事件并维护本地 state，与页面状态解耦，页面
// 重渲染不会拖累它，它也不会让页面跟随每个进度事件重渲染。
//
// 事件在 Rust 侧已按 ≥150ms 节流（见 download.rs PROGRESS_EMIT_MIN_INTERVAL_MS），
// 进度条不会因高频 IPC 抽搐；这里只做展示 + 取消入口，不参与模型状态管理。
//
// portal 到 body 的原因与 DownloadDialog 相同：WindowChrome 根节点的常驻
// transform / will-change 会为 position:fixed 后代创建 containing block，不
// portal 的话 fixed 会相对设置弹窗而非视口定位（灰屏 + 点不到的经典 bug）。

import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { isTauri } from '../lib/ipc';
import {
  cancelFoundryLocalAsrPrepare,
  cancelLocalAsrDownload,
  cancelSherpaOnnxAsrDownload,
  type FoundryPrepareProgress,
  type LocalAsrDownloadProgress,
} from '../lib/localAsr';
import { Icon } from './Icon';

type Engine = 'qwen3' | 'sherpa' | 'foundry';

interface ProgressItem {
  key: string;
  /** 引擎侧模型 id / alias（调用对应 cancel 命令用）。 */
  id: string;
  name: string;
  percent: number | null;
  engine: Engine;
}

const DOWNLOAD_TERMINAL_PHASES = new Set(['finished', 'cancelled', 'failed']);
const FOUNDRY_TERMINAL_PHASES = new Set(['finished', 'failed']);

export function GlobalDownloadProgress() {
  const { t } = useTranslation();
  const [items, setItems] = useState<Record<string, ProgressItem>>({});

  useEffect(() => {
    if (!isTauri) return;
    let unlistens: Array<() => void> = [];
    let cancelled = false;
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const qwenOff = await listen<LocalAsrDownloadProgress>(
        'local-asr-download-progress',
        (e) => {
          const p = e.payload;
          const key = `qwen3:${p.modelId}`;
          setItems((prev) => {
            if (DOWNLOAD_TERMINAL_PHASES.has(p.phase)) {
              const next = { ...prev };
              delete next[key];
              return next;
            }
            return {
              ...prev,
              [key]: {
                key,
                id: p.modelId,
                name: p.modelId,
                percent:
                  p.bytesTotal > 0
                    ? (p.bytesDownloaded / p.bytesTotal) * 100
                    : null,
                engine: 'qwen3' as const,
              },
            };
          });
        },
      );
      const sherpaOff = await listen<LocalAsrDownloadProgress>(
        'sherpa-onnx-asr-download-progress',
        (e) => {
          const p = e.payload;
          const key = `sherpa:${p.modelId}`;
          setItems((prev) => {
            if (DOWNLOAD_TERMINAL_PHASES.has(p.phase)) {
              const next = { ...prev };
              delete next[key];
              return next;
            }
            return {
              ...prev,
              [key]: {
                key,
                id: p.modelId,
                name: p.modelId,
                percent:
                  p.bytesTotal > 0
                    ? (p.bytesDownloaded / p.bytesTotal) * 100
                    : null,
                engine: 'sherpa' as const,
              },
            };
          });
        },
      );
      const foundryOff = await listen<FoundryPrepareProgress>(
        'foundry-local-asr-prepare-progress',
        (e) => {
          const p = e.payload;
          const key = `foundry:${p.modelAlias}`;
          setItems((prev) => {
            if (FOUNDRY_TERMINAL_PHASES.has(p.phase)) {
              const next = { ...prev };
              delete next[key];
              return next;
            }
            // phase 切换事件（runtime→model→load）不带进度，保留原条目不刷。
            if (p.percent == null) return prev;
            return {
              ...prev,
              [key]: {
                key,
                id: p.modelAlias,
                name: p.label || p.modelAlias,
                percent: p.percent,
                engine: 'foundry' as const,
              },
            };
          });
        },
      );
      if (cancelled) {
        qwenOff();
        sherpaOff();
        foundryOff();
      } else {
        unlistens = [qwenOff, sherpaOff, foundryOff];
      }
    })().catch((err) =>
      console.warn('[global-download-progress] subscribe failed', err),
    );
    return () => {
      cancelled = true;
      for (const off of unlistens) off();
    };
  }, []);

  const handleCancel = (item: ProgressItem) => {
    if (item.engine === 'qwen3') void cancelLocalAsrDownload(item.id);
    else if (item.engine === 'sherpa') void cancelSherpaOnnxAsrDownload(item.id);
    else void cancelFoundryLocalAsrPrepare();
  };

  const visible = Object.values(items);
  if (visible.length === 0) return null;

  return createPortal(
    <div
      style={{
        position: 'fixed',
        top: 14,
        right: 14,
        zIndex: 900,
        display: 'flex',
        flexDirection: 'column',
        gap: 8,
        maxWidth: 260,
      }}
    >
      {visible.map((item) => (
        <div
          key={item.key}
          style={{
            padding: '10px 12px',
            borderRadius: 10,
            background: 'var(--ol-surface)',
            border: '0.5px solid var(--ol-line-strong)',
            boxShadow: 'var(--ol-shadow-lg)',
            animation: 'ol-select-pop .18s var(--ol-motion-quick) both',
          }}
        >
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              gap: 8,
              fontSize: 11.5,
              color: 'var(--ol-ink)',
              marginBottom: 6,
            }}
          >
            <span
              style={{
                minWidth: 0,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {item.name}
            </span>
            <span
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 6,
                flexShrink: 0,
              }}
            >
              <span style={{ color: 'var(--ol-ink-4)' }}>
                {item.percent != null
                  ? `${Math.round(item.percent)}%`
                  : t('localAsr.downloading')}
              </span>
              <button
                type="button"
                onClick={() => handleCancel(item)}
                aria-label={t('common.cancel')}
                title={t('common.cancel')}
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  width: 18,
                  height: 18,
                  borderRadius: 5,
                  border: 0,
                  padding: 0,
                  background: 'transparent',
                  color: 'var(--ol-ink-3)',
                  cursor: 'pointer',
                  transition:
                    'background 0.12s var(--ol-motion-quick), color 0.12s var(--ol-motion-quick)',
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = 'var(--ol-surface-2)';
                  e.currentTarget.style.color = 'var(--ol-ink)';
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = 'transparent';
                  e.currentTarget.style.color = 'var(--ol-ink-3)';
                }}
              >
                <Icon name="close" size={10} />
              </button>
            </span>
          </div>
          <div
            style={{
              height: 5,
              borderRadius: 999,
              background: 'var(--ol-surface-2)',
              overflow: 'hidden',
            }}
          >
            <div
              style={{
                height: '100%',
                width: `${item.percent ?? 0}%`,
                background: 'var(--ol-blue)',
                transition: 'width 0.18s var(--ol-motion-soft)',
              }}
            />
          </div>
        </div>
      ))}
    </div>,
    document.body,
  );
}
