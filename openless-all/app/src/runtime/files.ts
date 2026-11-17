import { getRuntimeEnvironment } from './environment';
import { invokeOrPreview, requireBackendReady } from './transport';

const zipFilter = [{ name: 'Style Pack ZIP', extensions: ['zip'] }];

export async function selectStylePackZip(): Promise<string | null> {
  const { host } = getRuntimeEnvironment();
  if (host !== 'tauri') {
    return invokeOrPreview('pick_style_pack_zip', undefined, () => 'mock-style-pack.zip');
  }
  await requireBackendReady();
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({ filters: zipFilter, multiple: false });
  return typeof result === 'string' ? result : null;
}

export async function selectStylePackZipTarget(fileName: string): Promise<string | null> {
  try {
    const { host } = getRuntimeEnvironment();
    if (host !== 'tauri') {
      return await invokeOrPreview(
        'pick_style_pack_zip_target',
        { fileName },
        () => `~/Downloads/${fileName}`,
      );
    }
    await requireBackendReady();
    const { save } = await import('@tauri-apps/plugin-dialog');
    return await save({ defaultPath: fileName, filters: zipFilter });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (/\bcancel(?:l)?ed\b/i.test(message)) return null;
    throw error;
  }
}

export async function selectDirectory(options: {
  title?: string;
  previewPath?: string;
  prompt?: { label: string; value?: string };
}): Promise<string | null> {
  const { host } = getRuntimeEnvironment();
  if (host === 'preview' && options.previewPath !== undefined) return options.previewPath;
  if (host !== 'tauri') {
    // Harmony can accept a user-entered path but does not expose a directory picker.
    if (host === 'harmony') await requireBackendReady();
    return options.prompt ? window.prompt(options.prompt.label, options.prompt.value) : null;
  }
  await requireBackendReady();
  const { open } = await import('@tauri-apps/plugin-dialog');
  const result = await open({ directory: true, multiple: false, title: options.title });
  return typeof result === 'string' ? result : null;
}

export async function shareFiles(title: string, files: File[]): Promise<boolean> {
  const data = { title, files };
  if (!navigator.share || (navigator.canShare && !navigator.canShare(data))) return false;
  await navigator.share(data);
  return true;
}

export function downloadUrl(url: string, fileName: string): void {
  const link = document.createElement('a');
  link.href = url;
  link.download = fileName;
  document.body.append(link);
  try {
    link.click();
  } finally {
    link.remove();
  }
}
