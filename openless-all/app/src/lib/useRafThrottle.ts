import { useEffect, useRef, useState } from 'react';

/**
 * 返回 `value`，但每个动画帧最多更新一次。
 *
 * 用于流式文本（LLM token）等到达速度远高于刷新率的场景：把「每个 token 触发一次
 * 重渲染」坍缩成「每帧最多一次」，让昂贵的派生计算（markdown 全量解析、DOM 测量）
 * 按帧率（~60fps）而非 token 率运行。否则一段长回复的解析是 O(n²)（每来一个 token
 * 就把已累积的整段重新 parse 一遍）。
 *
 * 这是 throttle 而非 debounce：流式持续进行时每帧都会 flush 当前最新值；最新值最终
 * 一定会被投递（停止后的下一帧收尾），不会丢内容。
 */
export function useRafThrottle<T>(value: T): T {
  const [throttled, setThrottled] = useState<T>(value);
  const latest = useRef<T>(value);
  const frame = useRef<number | null>(null);
  latest.current = value;

  useEffect(() => {
    // 本帧已排程：不重排、不取消，最新值会在它触发时一并 flush。
    if (frame.current != null) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      setThrottled(latest.current);
    });
  }, [value]);

  // 仅在卸载时取消挂起的帧，避免对已卸载组件 setState。
  useEffect(
    () => () => {
      if (frame.current != null) cancelAnimationFrame(frame.current);
    },
    [],
  );

  return throttled;
}
