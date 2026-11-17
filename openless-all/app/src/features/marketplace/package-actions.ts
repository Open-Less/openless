import { logClientError, selectStylePackZipTarget } from '../../api/files';
import type { Interaction } from '../../api/interaction';
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
  confirm: Interaction['confirm'],
  options: Parameters<Interaction['confirm']>[0],
) {
  if (pack.authorLogin !== owner() || !(await confirm(options))) return false;
  if (pack.authorLogin !== owner()) return false;
  await marketplaceDelete(pack.id);
  return true;
}
