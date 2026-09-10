const INVALID_FILE_NAME_CHARS = /[<>:"/\\|?*\u0000-\u001f]+/g;

function normalizeFileNamePart(value: string): string {
  return value
    .trim()
    .replace(INVALID_FILE_NAME_CHARS, '-')
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^[.\-\s]+|[.\-\s]+$/g, '')
    .toLowerCase();
}

export function stylePackZipFileName(name: string, version?: string): string {
  const baseName = normalizeFileNamePart(name) || 'style-pack';
  const normalizedVersion = normalizeFileNamePart(version ?? '').replace(/^v(?=\d)/, '');
  const versionSuffix = normalizedVersion ? `-v${normalizedVersion}` : '';
  return `${baseName}${versionSuffix}.zip`;
}

export function isStylePackZipDialogCancellation(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /\bcancel(?:l)?ed\b/i.test(message);
}

export async function pickStylePackZipTargetPath(
  defaultFileName: string,
  nativeApp: boolean,
): Promise<string | null> {
  if (!nativeApp) return `~/Downloads/${defaultFileName}`;

  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    return await save({
      defaultPath: defaultFileName,
      filters: [{ name: 'Style Pack ZIP', extensions: ['zip'] }],
    });
  } catch (error) {
    if (isStylePackZipDialogCancellation(error)) return null;
    throw error;
  }
}
