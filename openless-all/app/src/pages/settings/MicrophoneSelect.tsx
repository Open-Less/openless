// Microphone select — reuses the SelectLite "official box" dropdown: opening pops a microphone list where
// the selected item gets a check on the far right with a live level meter to its left. While the dropdown is
// open, the level of the selected device is monitored.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isTauri, startMicrophoneLevelMonitor, stopMicrophoneLevelMonitor } from '../../lib/ipc';
import type { MicrophoneDevice } from '../../lib/types';
import { SelectLite, type SelectOption } from '../../components/ui/SelectLite';
import { useMobileLayout } from '../../lib/useMobileLayout';

interface MicrophoneSelectProps {
  devices: MicrophoneDevice[];
  /** Currently effective device name; '' = system default. */
  selectedName: string;
  onSelect: (name: string) => void;
  /** Callback when the dropdown opens — used to refresh the device list. */
  onOpen?: () => void;
}

export function MicrophoneSelect({
  devices,
  selectedName,
  onSelect,
  onOpen,
}: MicrophoneSelectProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const [open, setOpen] = useState(false);
  const [level, setLevel] = useState(0);
  // Serialize start/stop — prevents listener / Rust-side state mismatch on rapid open-close.
  const monitorQueueRef = useRef<Promise<void>>(Promise.resolve());

  const enqueueMonitorTask = useCallback((task: () => Promise<void>) => {
    const next = monitorQueueRef.current.catch(() => undefined).then(task);
    monitorQueueRef.current = next.catch(() => undefined);
    return next;
  }, []);

  // Monitor the selected device's level while the dropdown is open; stop and reset on close.
  useEffect(() => {
    if (!open) {
      setLevel(0);
      return;
    }
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let timer: number | undefined;

    async function start() {
      await enqueueMonitorTask(async () => {
        try {
          if (isTauri) {
            const { listen } = await import('@tauri-apps/api/event');
            if (cancelled) return;
            const stopListening = await listen<{ level: number }>('microphone:level', (event) => {
              setLevel(Math.max(0, Math.min(1, event.payload.level ?? 0)));
            });
            if (cancelled) {
              stopListening();
              return;
            }
            unlisten = stopListening;
            await startMicrophoneLevelMonitor(selectedName);
            if (cancelled) {
              unlisten?.();
              unlisten = undefined;
              await stopMicrophoneLevelMonitor();
            }
          } else {
            const tick = window.setInterval(() => {
              setLevel(0.25 + Math.random() * 0.55);
            }, 120);
            if (cancelled) {
              window.clearInterval(tick);
              return;
            }
            unlisten = () => window.clearInterval(tick);
          }
        } catch (err) {
          console.warn('[settings] microphone level monitor failed', err);
        }
      });
    }

    timer = window.setTimeout(() => {
      void start();
    }, 140);
    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
      void enqueueMonitorTask(async () => {
        unlisten?.();
        unlisten = undefined;
        await stopMicrophoneLevelMonitor();
      });
    };
  }, [enqueueMonitorTask, open, selectedName]);

  // The selected entry (default mic or a device row) carries the level meter on its right; SelectLite appends the check after it.
  const options = useMemo<SelectOption[]>(() => {
    const meter = <LevelMeter level={level} />;
    return [
      {
        value: '',
        label: t('settings.recording.microphoneDefault'),
        trailing: selectedName === '' ? meter : undefined,
      },
      ...devices.map((device) => ({
        value: device.name,
        label: device.name,
        trailing: selectedName === device.name ? meter : undefined,
      })),
    ];
  }, [devices, level, selectedName, t]);

  return (
    <SelectLite
      value={selectedName}
      onChange={onSelect}
      options={options}
      ariaLabel={t('settings.recording.microphoneLabel')}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) onOpen?.();
      }}
      style={{ width: mobile ? '100%' : 200, maxWidth: '100%', minWidth: 0 }}
    />
  );
}

function LevelMeter({ level }: { level: number }) {
  const amplified = Math.min(1, Math.max(0, level * 4.5));
  const bars = [0.4, 0.7, 1, 0.7, 0.4];
  return (
    <span
      style={{ display: 'inline-flex', alignItems: 'center', gap: 3, height: 14, flexShrink: 0 }}
    >
      {bars.map((weight, index) => {
        const intensity = Math.min(1, amplified * (0.85 + weight * 0.35));
        const height = 4 + intensity * (10 * weight);
        return (
          <span
            key={`${weight}-${index}`}
            style={{
              width: 3,
              height,
              borderRadius: 999,
              background: intensity > 0.08 ? 'var(--ol-blue)' : 'rgba(0,0,0,0.14)',
              opacity: 0.4 + intensity * 0.6,
              transition: 'height 70ms linear, opacity 90ms ease, background 120ms ease',
            }}
          />
        );
      })}
    </span>
  );
}
