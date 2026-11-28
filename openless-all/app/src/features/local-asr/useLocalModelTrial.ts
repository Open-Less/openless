import { useEffect, useRef, useState } from 'react';
import { activateLocalAsr, testLocalAsrModel, type LocalAsrTestResult } from '../../api/local-asr';

export type TrialProvider = 'local-qwen3-mlx' | 'local-qwen3-c' | 'local-whisper';

export function useLocalModelTrial(
  refresh: () => Promise<unknown>,
  onActivationError: (error: string) => void,
) {
  const request = useRef<symbol | null>(null);
  const [testingModelId, setTestingModelId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<
    Record<string, LocalAsrTestResult | { error: string }>
  >({});
  useEffect(
    () => () => {
      request.current = null;
    },
    [],
  );

  const runTrial = async (modelId: string, provider: TrialProvider) => {
    if (request.current) return;
    const token = Symbol('local model trial');
    request.current = token;
    const current = () => request.current === token;
    let activated = false;
    setTestingModelId(modelId);
    setTestResults((previous) => {
      const next = { ...previous };
      delete next[modelId];
      return next;
    });
    try {
      // Activation must commit before the backend can transcribe using this model.
      await activateLocalAsr('generic', modelId, provider);
      if (!current()) return;
      await refresh();
      if (!current()) return;
      activated = true;
      const result = await testLocalAsrModel(modelId);
      if (current()) setTestResults((previous) => ({ ...previous, [modelId]: result }));
    } catch (error) {
      if (!current()) return;
      const message = error instanceof Error ? error.message : String(error);
      if (activated) setTestResults((previous) => ({ ...previous, [modelId]: { error: message } }));
      else onActivationError(message);
    } finally {
      if (current()) {
        request.current = null;
        setTestingModelId(null);
      }
    }
  };
  return { testingModelId, testResults, runTrial };
}
