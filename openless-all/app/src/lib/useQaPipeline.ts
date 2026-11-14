import { useEffect, useState } from 'react';
import { getSettings } from './ipc';
import { isTauri } from './ipc/shared';
import type { PipelineMode } from './types';
export function useQaPipeline(): PipelineMode {
  const [mode, setMode] = useState<PipelineMode>('traditional');
  useEffect(() => {
    let active = true,
      revision = 0;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      const token = ++revision;
      try {
        const prefs = await getSettings();
        if (active && token === revision)
          setMode(
            prefs.pipelineMode === 'multimodal' && !prefs.multimodalPipelineEnabled
              ? 'traditional'
              : prefs.pipelineMode,
          );
      } catch {
        if (active && token === revision) setMode('traditional');
      }
    };
    void refresh();
    if (isTauri)
      void import('@tauri-apps/api/event').then(async ({ listen }) => {
        const stop = await listen('prefs:changed', () => void refresh());
        if (active) {
          unlisten = stop;
          void refresh();
        } else stop();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  return mode;
}
