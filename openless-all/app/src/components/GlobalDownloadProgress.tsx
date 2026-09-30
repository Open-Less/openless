// GlobalDownloadProgress.tsx — global download progress overlay.
//
// Persistently visible on every main-window page (portaled to document.body, fixed
// top-right): appears when a download starts and stays until it ends; page switches
// and settings-modal toggles don't affect it. Self-contained — it subscribes to the
// three local ASR engines' progress events and keeps its own state, decoupled from
// page state: page re-renders don't drag it, and it doesn't re-render pages on every
// progress event.
//
// Events are already throttled to ≥150ms in Core's ModelStore, so the bar doesn't
// jitter on high-frequency IPC; this component only displays + offers cancel, it
// doesn't manage model state.
//
// Same portal-to-body reason as DownloadDialog: WindowChrome's root carries persistent
// transform / will-change, creating a containing block for position:fixed descendants;
// without the portal, fixed anchors to the settings dialog instead of the viewport
// (the classic gray screen + unclickable bug).

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
  /** Engine-side model id / alias (for calling the matching cancel command). */
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
      const qwenOff = await listen<LocalAsrDownloadProgress>('local-asr-download-progress', (e) => {
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
              percent: p.bytesTotal > 0 ? (p.bytesDownloaded / p.bytesTotal) * 100 : null,
              engine: 'qwen3' as const,
            },
          };
        });
      });
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
                percent: p.bytesTotal > 0 ? (p.bytesDownloaded / p.bytesTotal) * 100 : null,
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
            // Phase-switch events (runtime→model→load) carry no progress; keep the
            // existing entry unrefreshed.
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
    })().catch((err) => console.warn('[global-download-progress] subscribe failed', err));
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
                {item.percent != null ? `${Math.round(item.percent)}%` : t('localAsr.downloading')}
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
