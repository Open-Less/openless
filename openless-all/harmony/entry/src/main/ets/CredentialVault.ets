import { asset } from '@kit.AssetStoreKit';
import { util } from '@kit.ArkTS';
import { BusinessError } from '@kit.BasicServicesKit';
import nativeHost from 'libopenless_harmony.so';

interface CredentialRecord { account: string; provider?: string; value?: string; }

export async function restoreHarmonyCredentials(): Promise<number> {
  try {
    const query = new Map<asset.Tag, asset.Value>();
    query.set(asset.Tag.DATA_LABEL_NORMAL_1, new util.TextEncoder().encodeInto('openless.credentials'));
    query.set(asset.Tag.RETURN_TYPE, asset.ReturnType.ATTRIBUTES);
    const records = await asset.query(query);
    for (const record of records) {
      const args: CredentialRecord = JSON.parse(util.TextDecoder.create('utf-8').decodeWithStream(record.get(asset.Tag.ALIAS) as Uint8Array));
      const secretQuery = new Map<asset.Tag, asset.Value>();
      secretQuery.set(asset.Tag.ALIAS, record.get(asset.Tag.ALIAS) as Uint8Array);
      secretQuery.set(asset.Tag.RETURN_TYPE, asset.ReturnType.ALL);
      const secrets = await asset.query(secretQuery);
      args.value = util.TextDecoder.create('utf-8').decodeWithStream(secrets[0].get(asset.Tag.SECRET) as Uint8Array);
      const wire = await nativeHost.invokeAsync('__hydrate_credential', JSON.stringify(args));
      const result: Record<string, Object> = JSON.parse(wire);
      if (result['ok'] !== true) throw new Error('凭据恢复失败');
    }
    console.info('[OpenLess Vault] secure credentials restored');
    return records.length;
  } catch (error) {
    if ((error as BusinessError).code !== 24000002) throw new Error(String(error));
    return 0;
  }
}
