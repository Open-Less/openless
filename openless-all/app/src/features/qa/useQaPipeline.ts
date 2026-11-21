import { useEffect, useState } from 'react';
import type { PipelineMode } from '../../api/contracts';
import { getSettings, listenPreferenceChanges } from '../../api/preferences';

export function useQaPipeline(): PipelineMode {
  const [mode, setMode] = useState<PipelineMode>('traditional');
  useEffect(() => {
    let active = true;
    let revision = 0;
    let unlisten: (() => void) | undefined;
    const refresh = () => {
      const token = ++revision;
      void getSettings().then(
        (prefs) => {
          if (active && token === revision)
            setMode(
              prefs.pipelineMode === 'multimodal' && !prefs.multimodalPipelineEnabled
                ? 'traditional'
                : prefs.pipelineMode,
            );
        },
        () => {
          if (active && token === revision) setMode('traditional');
        },
      );
    };
    void listenPreferenceChanges(refresh).then(
      (stop) => {
        if (!active) stop();
        else {
          unlisten = stop;
          refresh();
        }
      },
      () => {
        if (active) refresh();
      },
    );
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  return mode;
}
