import { copyTextToClipboard as copyNative } from '../lib/ipc/vocab';
import { isPreviewHost } from '../runtime/environment';

export {
  selectDirectory,
  selectStylePackZip,
  selectStylePackZipTarget,
  shareFiles,
} from '../runtime/files';
export {
  openExternal,
  exportErrorLog,
  logClientError,
  debugReadCursorContext,
} from '../lib/ipc/utils';
export { importStylePackFromZip, exportStylePackToZip } from '../lib/ipc/style-packs';
export { openAndroidLocalMigration } from '../lib/ipc/local-migration';

export async function copyTextToClipboard(text: string): Promise<void> {
  if (!isPreviewHost()) return copyNative(text);
  await navigator.clipboard.writeText(text);
}
