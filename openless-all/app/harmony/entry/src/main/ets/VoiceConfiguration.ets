import { asset } from '@kit.AssetStoreKit';
import { util } from '@kit.ArkTS';
import nativeHost from 'libopenless_harmony.so';

interface SnapshotReply { ok: boolean; value: Object; }
interface SnapshotManifest { generation: string; chunks: number; bytes: number; }
const label = 'openless.voice.configuration.v1';
const manifestAlias = label + '.manifest';
let published = '';
let sequence = 0;
function bytes(text: string): Uint8Array { return new util.TextEncoder().encodeInto(text); }
function text(value: Uint8Array): string { return util.TextDecoder.create('utf-8').decodeWithStream(value); }
function query(alias: string): Map<asset.Tag, asset.Value> {
  const result = new Map<asset.Tag, asset.Value>();
  result.set(asset.Tag.ALIAS, bytes(alias));
  result.set(asset.Tag.RETURN_TYPE, asset.ReturnType.ALL);
  return result;
}
async function read(alias: string): Promise<Uint8Array> {
  const records = await asset.query(query(alias));
  if (records.length !== 1) throw new Error('语音配置尚未同步，请打开 OpenLess 主应用');
  return records[0].get(asset.Tag.SECRET) as Uint8Array;
}
async function write(alias: string, secret: Uint8Array): Promise<void> {
  const attributes = new Map<asset.Tag, asset.Value>();
  attributes.set(asset.Tag.ALIAS, bytes(alias));
  attributes.set(asset.Tag.SECRET, secret);
  attributes.set(asset.Tag.DATA_LABEL_NORMAL_1, bytes(label));
  attributes.set(asset.Tag.ACCESSIBILITY, asset.Accessibility.DEVICE_UNLOCKED);
  attributes.set(asset.Tag.CONFLICT_RESOLUTION, asset.ConflictResolution.OVERWRITE);
  await asset.add(attributes);
}

// Only the main app publishes. Secrets remain in the existing credential vault.
export async function publishVoiceConfiguration(): Promise<void> {
  const reply: SnapshotReply = JSON.parse(await nativeHost.invokeAsync('__export_voice_configuration', '{}'));
  if (!reply.ok) throw new Error('语音配置读取失败');
  const snapshot = JSON.stringify(reply.value);
  if (snapshot === published) return;
  const payload = bytes(snapshot);
  if (!payload.length || payload.length > 65536) throw new Error('语音配置超过同步大小限制');
  const generation = Date.now().toString() + '-' + (++sequence).toString();
  const chunks = Math.ceil(payload.length / 1024);
  for (let index = 0; index < chunks; index++) {
    await write(label + '.' + generation + '.' + index.toString(), payload.slice(index * 1024, (index + 1) * 1024));
  }
  const manifest: SnapshotManifest = { generation: generation, chunks: chunks, bytes: payload.length };
  // Publish the pointer last. Readers never accept an incomplete generation.
  await write(manifestAlias, bytes(JSON.stringify(manifest)));
  published = snapshot;
  const oldQuery = new Map<asset.Tag, asset.Value>();
  oldQuery.set(asset.Tag.DATA_LABEL_NORMAL_1, bytes(label));
  oldQuery.set(asset.Tag.RETURN_TYPE, asset.ReturnType.ATTRIBUTES);
  try {
    for (const record of await asset.query(oldQuery)) {
      const alias = text(record.get(asset.Tag.ALIAS) as Uint8Array);
      if (alias !== manifestAlias && !alias.startsWith(label + '.' + generation + '.')) {
        const remove = new Map<asset.Tag, asset.Value>();
        remove.set(asset.Tag.ALIAS, bytes(alias));
        await asset.remove(remove);
      }
    }
  } catch (_) { console.warn('[OpenLess Voice Config] old snapshot cleanup deferred'); }
}

export async function restoreVoiceConfiguration(): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const before = text(await read(manifestAlias));
      const manifest: SnapshotManifest = JSON.parse(before);
      if (manifest.bytes < 1 || manifest.bytes > 65536 || manifest.chunks !== Math.ceil(manifest.bytes / 1024) ||
        !/^[0-9]+-[0-9]+$/.test(manifest.generation)) throw new Error('语音配置快照无效');
      const payload = new Uint8Array(manifest.bytes);
      for (let index = 0; index < manifest.chunks; index++) {
        const chunk = await read(label + '.' + manifest.generation + '.' + index.toString());
        if (chunk.length !== Math.min(1024, manifest.bytes - index * 1024)) throw new Error('语音配置快照不完整');
        payload.set(chunk, index * 1024);
      }
      if (text(await read(manifestAlias)) !== before) continue;
      const reply: SnapshotReply = JSON.parse(await nativeHost.invokeAsync('__import_voice_configuration', text(payload)));
      if (!reply.ok) throw new Error('语音配置应用失败');
      console.info('[OpenLess Voice Config] main application configuration restored');
      return;
    } catch (error) {
      if (attempt === 2) throw new Error('语音配置同步失败，请先打开 OpenLess 主应用');
    }
  }
  throw new Error('语音配置正在更新，请重试');
}
