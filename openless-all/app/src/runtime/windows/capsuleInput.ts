import type { CapsuleInputRect } from '../../lib/capsuleInputRegions';
import { getRuntimeEnvironment } from '../environment';
import { invokeOrPreview } from '../transport';

// Only the macOS capsule and its mirrors register this command in the current host.
export function setCapsuleInteractiveRegions(rects: CapsuleInputRect[]): Promise<void> {
  const { host, os } = getRuntimeEnvironment();
  if (host !== 'tauri' || os !== 'mac') return Promise.resolve();
  return invokeOrPreview('set_capsule_interactive_regions', { rects }, () => undefined);
}
