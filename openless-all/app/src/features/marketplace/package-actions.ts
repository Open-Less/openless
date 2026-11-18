import { logClientError, selectStylePackZipTarget } from '../../api/files';
import {
  downloadMarketplacePack,
  installMarketplacePack,
  marketplaceDelete,
} from '../../api/marketplace';
import { stylePackZipFileName } from '../../lib/stylePackZip';

export async function installListedPack(packId: string): Promise<void> {
  try {
    await installMarketplacePack(packId);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const clientFailureLog = `[marketplace-install] stage=ipc-failed pack_id=${packId} error=${message}`;
    // Logging is best effort and must never keep the package's install state busy.
    void logClientError(clientFailureLog);
    throw error;
  }
}

export async function downloadListedPack(pack: { id: string; name: string; version: string }) {
  const target = await selectStylePackZipTarget(stylePackZipFileName(pack.name, pack.version));
  if (target === null) return false;
  await downloadMarketplacePack(pack.id, target);
  return true;
}

export async function withdrawListedPack(
  pack: { id: string; authorLogin: string },
  owner: () => string | null,
) {
  if (pack.authorLogin !== owner()) throw new Error('Marketplace account changed');
  await marketplaceDelete(pack.id);
}
