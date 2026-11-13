// 录音提示音：监听 capsule:state 事件，在"开始录音"边沿播放合成提示音。
// 独立组件，不依赖胶囊窗口显示——胶囊隐藏时也能正常工作。
// Web Audio 输出会被某些桌面音频栈当作 sink-input，启动/退出时触发音量 OSD；
// Android 也可能因 Web Audio 输出切换录音输入路由，因此这两个平台禁用。

import { useEffect, useRef } from 'react';
import { isAndroid, isTauri } from '../lib/ipc';
import { disposeAudioCue, playRecordStartCue, stopAudioCue } from '../lib/audioCue';
import type { CapsuleState, UserPreferences } from '../lib/types';

interface CapsulePayload {
  state: CapsuleState;
  level?: number;
  message?: string | null;
  insertedChars?: number | null;
  translation?: boolean;
}

export function AudioCueListener() {
  const audioCueEnabledRef = useRef(false);
  const audioCuePreferenceChangedRef = useRef(false);
  const prevStateRef = useRef<CapsuleState>('idle' as CapsuleState);
  const audioCueRuntimeEnabled = !isAndroid();

  // Read settings before playing cues.
  useEffect(() => {
    if (!isTauri || !audioCueRuntimeEnabled) return;
    let cancelled = false;
    (async () => {
      try {
        const { getSettings } = await import('../lib/ipc');
        const prefs = await getSettings();
        if (cancelled || audioCuePreferenceChangedRef.current) return;
        audioCueEnabledRef.current = prefs.audioCueOnRecord !== false;
      } catch {
        // Leave the cue off until the preference is known.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [audioCueRuntimeEnabled]);

  // Listen for settings changes
  useEffect(() => {
    if (!isTauri || !audioCueRuntimeEnabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      listen<UserPreferences>('prefs:changed', (event) => {
        const next = event.payload;
        if (cancelled || !next) return;
        audioCuePreferenceChangedRef.current = true;
        audioCueEnabledRef.current = next.audioCueOnRecord !== false;
        if (!audioCueEnabledRef.current) disposeAudioCue();
      })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => {});
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [audioCueRuntimeEnabled]);

  // Listen for capsule state edges
  useEffect(() => {
    if (!isTauri || !audioCueRuntimeEnabled) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
      if (cancelled) return;
      const currentWindow = getCurrentWebviewWindow();
      // The cue belongs to the resident recording capsule only. The main window must not
      // replay the current recording edge when it is opened during an active session.
      if (currentWindow.label !== 'capsule') return;
      const target = { kind: 'WebviewWindow' as const, label: currentWindow.label };
      listen<CapsulePayload>(
        'capsule:state',
        (event) => {
          if (cancelled) return;
          const state = event.payload.state;
          const prev = prevStateRef.current;
          prevStateRef.current = state;
          if (state === 'recording' && prev !== 'recording') {
            if (audioCueEnabledRef.current) playRecordStartCue();
          } else if (state !== 'recording' && prev === 'recording') {
            stopAudioCue();
          }
        },
        { target },
      )
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => {});
    })();
    return () => {
      cancelled = true;
      unlisten?.();
      disposeAudioCue();
    };
  }, [audioCueRuntimeEnabled]);

  return null;
}
