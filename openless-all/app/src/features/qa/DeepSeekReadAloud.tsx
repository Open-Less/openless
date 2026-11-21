import { useEffect, useRef, useState } from 'react';
import { startQaReadAloud, stopQaReadAloud } from '../../api/qa';
import { useTranslation } from 'react-i18next';
import { isTauriHost } from '../../runtime/environment';

let stopActiveReading: (() => void) | null = null;

export function stopDeepSeekReading() {
  stopActiveReading?.();
}

export function DeepSeekReadAloud({ text }: { text: string }) {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith('zh');
  const [playing, setPlaying] = useState(false);
  const [error, setError] = useState('');
  const context = useRef<AudioContext | null>(null);
  const generation = useRef(0);
  const requestId = useRef('');
  const activeStop = useRef<(() => void) | null>(null);
  const finishTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const stop = () => {
    if (stopActiveReading === activeStop.current) stopActiveReading = null;
    activeStop.current = null;
    if (finishTimer.current !== null) clearTimeout(finishTimer.current);
    finishTimer.current = null;
    generation.current++;
    void context.current?.close().catch(() => {});
    context.current = null;
    setPlaying(false);
    void stopQaReadAloud(requestId.current).catch(() => {});
  };
  useEffect(
    () => () => {
      if (context.current) stop();
    },
    [text],
  );
  if (!isTauriHost()) return null;
  return (
    <>
      <button
        type="button"
        className="qa-copy qa-read-aloud"
        aria-pressed={playing}
        aria-label={
          playing ? (zh ? '停止朗读' : 'Stop reading') : zh ? 'DeepSeek 朗读' : 'Read with DeepSeek'
        }
        onClick={async () => {
          if (playing) {
            stop();
            return;
          }
          stopActiveReading?.();
          stopActiveReading = stop;
          activeStop.current = stop;
          const id = crypto.randomUUID();
          requestId.current = id;
          const epoch = ++generation.current;
          try {
            const ctx = new AudioContext({ sampleRate: 24000 });
            context.current = ctx;
            setPlaying(true);
            setError('');
            let cursor = 0;
            let pending: number | null = null;
            const onAudio = (data: number[]) => {
              if (generation.current !== epoch) return;
              const bytes = pending === null ? data : [pending, ...data];
              pending = bytes.length % 2 ? bytes[bytes.length - 1] : null;
              const count = Math.floor(bytes.length / 2);
              if (!count) return;
              const buffer = ctx.createBuffer(1, count, 24000);
              const samples = buffer.getChannelData(0);
              for (let i = 0; i < count; i++) {
                const n = bytes[i * 2] | (bytes[i * 2 + 1] << 8);
                samples[i] = (n >= 32768 ? n - 65536 : n) / 32768;
              }
              const source = ctx.createBufferSource();
              source.buffer = buffer;
              source.connect(ctx.destination);
              cursor = Math.max(cursor, ctx.currentTime + 0.03);
              source.onended = () => source.disconnect();
              source.start(cursor);
              cursor += buffer.duration;
            };
            await ctx.resume();
            // Device activation can finish after Stop, dismissal, or a newer playback.
            if (generation.current !== epoch) return;
            await startQaReadAloud(text, id, onAudio);
            if (generation.current !== epoch) return;
            if (pending !== null) throw new Error('Incomplete PCM frame');
            finishTimer.current = setTimeout(
              () => {
                if (generation.current === epoch) stop();
              },
              Math.max(0, cursor - ctx.currentTime) * 1000,
            );
          } catch {
            if (generation.current === epoch) {
              stop();
              setError(
                zh
                  ? '朗读失败，请检查模型服务中的 DeepSeek 配置。'
                  : 'Read aloud failed. Check DeepSeek in Model services.',
              );
            }
          }
        }}
      >
        {playing ? (zh ? '停止' : 'Stop') : zh ? '朗读' : 'Read aloud'}
      </button>
      {error && <span role="alert">{error}</span>}
    </>
  );
}
