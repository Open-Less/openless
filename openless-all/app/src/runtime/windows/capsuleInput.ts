import type { CapsuleInputRect } from '../../lib/capsuleInputRegions';
import { getRuntimeEnvironment } from '../environment';
import { invokeOrPreview } from '../transport';

declare global {
  interface Window {
    __openlessSetInputRegions?: (rects: CapsuleInputRect[]) => void;
  }
}

// Linux input regions belong to the current native view, not to a global HTTP
// command: the caption rail must never overwrite another surface's hit region.
export function setCapsuleInteractiveRegions(rects: CapsuleInputRect[]): Promise<void> {
  const { host, os } = getRuntimeEnvironment();
  if (host === 'linux') {
    if (!window.__openlessSetInputRegions)
      return Promise.reject(new Error('Native input-region bridge unavailable'));
    window.__openlessSetInputRegions(rects);
    return Promise.resolve();
  }
  if (host !== 'tauri' || os !== 'mac') return Promise.resolve();
  return invokeOrPreview('set_capsule_interactive_regions', { rects }, () => undefined);
}

export function createCapsuleRegionPublisher(write = setCapsuleInteractiveRegions) {
  let version = 0;
  let tail = Promise.resolve();
  return (rects: CapsuleInputRect[]) => {
    const next = ++version;
    const result = tail
      .catch(() => undefined)
      .then(async () => {
        // Native calls already in flight cannot be cancelled. Apply the newest
        // observed frame afterward so a former menu cannot restore its hit area.
        if (next === version) await write(rects);
      });
    tail = result;
    return result;
  };
}
