import { useEffect, useRef } from 'react';
import { createCapsuleSubscriptions } from '../../api/dictation';
import { getSettings } from '../../api/preferences';
import { getRuntimeEnvironment } from '../../runtime/environment';
import { disposeAudioCue, playRecordStartCue, stopAudioCue } from '../../lib/audioCue';
import type { CapsuleState } from '../../api/contracts';

export function AudioCueListener() {
  const audioCueEnabledRef = useRef(true);
  const prevStateRef = useRef<CapsuleState>('idle');
  const { host, os } = getRuntimeEnvironment();
  const audioCueRuntimeEnabled = host === 'tauri' && os !== 'android';

  useEffect(() => {
    if (!audioCueRuntimeEnabled) return;
    const events = createCapsuleSubscriptions();
    let disposed = false;
    let owned = false;
    let preferenceRevision = 0;

    void (async () => {
      // Opening another WebView must never play a cue for an existing session.
      if ((await events.label()) !== 'capsule' || disposed) return;
      owned = true;
      prevStateRef.current = 'idle';
      if (
        !(await events.preferences((next) => {
          preferenceRevision += 1;
          if (next) {
            audioCueEnabledRef.current = next.audioCueOnRecord !== false;
            if (!audioCueEnabledRef.current) disposeAudioCue();
          }
        }))
      )
        return;
      if (
        !(await events.state(({ state }) => {
          const previous = prevStateRef.current;
          prevStateRef.current = state;
          if (state === 'recording' && previous !== 'recording') {
            if (audioCueEnabledRef.current) playRecordStartCue();
          } else if (state !== 'recording' && previous === 'recording') {
            stopAudioCue();
          }
        }))
      )
        return;
      const observed = preferenceRevision;
      const preferences = await getSettings();
      if (!disposed && observed === preferenceRevision) {
        audioCueEnabledRef.current = preferences.audioCueOnRecord !== false;
        if (!audioCueEnabledRef.current) disposeAudioCue();
      }
    })().catch(() => {});

    return () => {
      disposed = true;
      events.dispose();
      if (owned) disposeAudioCue();
    };
  }, [audioCueRuntimeEnabled]);

  return null;
}
