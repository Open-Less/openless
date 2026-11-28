import { getRuntimeOS, isTauriHost } from '../runtime/environment';
import { invokeOrPreview } from '../runtime/transport';

export * from '../runtime/platform';

/** Sync the native Windows caption with the in-app dark/light theme. */
export async function syncWindowsCaptionTheme(dark: boolean): Promise<void> {
  if (!isTauriHost() || getRuntimeOS() !== 'win') return;
  try {
    await invokeOrPreview('set_windows_caption_theme', { dark }, () => undefined);
  } catch (error) {
    console.warn('[platform] set_windows_caption_theme failed', error);
  }
}
