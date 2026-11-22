import type { CapsuleInputRect } from '../../lib/capsuleInputRegions';
import { getRuntimeEnvironment } from '../environment';
import { invokeOrPreview } from '../transport';

// Only the macOS capsule and its mirrors register this command in the current host.
export function setCapsuleInteractiveRegions(rects: CapsuleInputRect[]): Promise<void> {
  const { host, os } = getRuntimeEnvironment();
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
