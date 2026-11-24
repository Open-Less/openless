import { selectStylePackZip, selectStylePackZipTarget } from '../../api/files';
import { exportStylePackToZip, importStylePackFromZip } from '../../api/style-packs';
import { stylePackZipFileName } from '../../lib/stylePackZip';

export async function importSelectedStylePack() {
  const archive = await selectStylePackZip();
  return archive === null ? null : importStylePackFromZip(archive);
}

export async function exportSelectedStylePack(id: string, name: string) {
  const target = await selectStylePackZipTarget(stylePackZipFileName(name));
  return target === null ? null : exportStylePackToZip(id, target);
}
