import { useCallback, useEffect, useLayoutEffect, useRef, type RefObject } from 'react';
import {
  mainCaptionTraceEnabled,
  reportMainCaptionTrace,
  type CaptionTraceStage,
} from '../../api/dictation';
import type { BackendEvent, TranscriptViewState } from '../../lib/backendEvent';

type Receipt = { sessionId: string; sequence: number; generation: number };
const sampleIntervalMs = 1_000;

export function useMainCaptionTrace(
  transcript: RefObject<TranscriptViewState>,
  visibleText: string,
) {
  const enabled = useRef(false);
  const accepted = useRef<Receipt>();
  const lastReported = useRef<
    Partial<Record<CaptionTraceStage, { sessionId: string; at: number }>>
  >({});

  useEffect(() => {
    let alive = true;
    void mainCaptionTraceEnabled()
      .then((value) => {
        if (alive) enabled.current = value === true;
      })
      .catch(() => {});
    return () => {
      alive = false;
      enabled.current = false;
    };
  }, []);

  const report = useCallback((stage: CaptionTraceStage, receipt: Receipt) => {
    const at = performance.now();
    const last = lastReported.current[stage];
    if (last?.sessionId === receipt.sessionId && at - last.at < sampleIntervalMs) return;
    lastReported.current[stage] = { sessionId: receipt.sessionId, at };
    void reportMainCaptionTrace(receipt.sessionId, receipt.sequence, stage).catch(() => {});
  }, []);

  useLayoutEffect(() => {
    if (!enabled.current || !visibleText) return;
    const receipt = accepted.current;
    const isCurrent = () => {
      const current = transcript.current;
      return (
        receipt &&
        current?.sessionId === receipt.sessionId &&
        current.generation === receipt.generation &&
        current.text.trim() === visibleText
      );
    };
    if (!receipt || !isCurrent()) return;
    // This runs after a React commit, at the browser's next frame callback.
    // It cannot attest that the OS displayed or composited the window.
    const frame = requestAnimationFrame(() => {
      if (enabled.current && isCurrent()) report('render_callback', receipt);
    });
    return () => cancelAnimationFrame(frame);
  }, [report, transcript, visibleText]);

  return useCallback(
    (event: BackendEvent, before: TranscriptViewState, after: TranscriptViewState) => {
      if (
        !enabled.current ||
        event.kind.type !== 'transcript_delta' ||
        !event.sessionId ||
        !Number.isSafeInteger(event.sequence) ||
        event.sequence <= 0
      )
        return;
      const receipt = {
        sessionId: event.sessionId,
        sequence: event.sequence,
        generation: after.generation,
      };
      if (
        after !== before &&
        after.sessionId === event.sessionId &&
        after.sequence === event.sequence
      ) {
        accepted.current = receipt;
        report('accepted', receipt);
      } else {
        report('rejected', receipt);
      }
    },
    [report],
  );
}
