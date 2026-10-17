import { resourceManager } from '@kit.LocalizationKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import nativeHost from 'libopenless_harmony.so';
import { util } from '@kit.ArkTS';

export interface RimeState {
  ok: boolean;
  error?: string;
  session: string;
  revision: number;
  input: string;
  preedit: string;
  candidates: string[];
  commit: string;
  page: number;
  hasNext: boolean;
}
let initialization: Promise<void> | undefined = undefined;
let validated: boolean = false;

export async function rimeRequest(operation: string, session: string = '', argument: string = ''): Promise<RimeState> {
  const state: RimeState = JSON.parse(await nativeHost.rimeAsync(operation, session, argument));
  if (!state.ok) throw new Error(state.error || 'Rime 请求失败');
  return state;
}

interface CloudReply { ok: boolean; value?: string[]; }
export async function cloudCandidates(filesDir: string, cacheDir: string, pinyin: string): Promise<string[]> {
  const initialized: CloudReply = JSON.parse(nativeHost.initBackend(filesDir + '/openless', cacheDir + '/openless'));
  if (!initialized.ok) return [];
  const reply: CloudReply = JSON.parse(await nativeHost.invokeAsync('ime_cloud_candidates', JSON.stringify({ pinyin: pinyin })));
  return reply.ok ? reply.value || [] : [];
}

export function initializeRime(filesDir: string, resources: resourceManager.ResourceManager, profile: string = 'rime-user'): Promise<void> {
  if (initialization) return initialization;
  initialization = (async (): Promise<void> => {
    const shared = filesDir + '/rime-shared-1.17.0-ice-da1fbe60-v1';
    if (!fs.accessSync(shared)) fs.mkdirSync(shared);
    const decoder = util.TextDecoder.create('utf-8');
    const files: string[] = JSON.parse(decoder.decodeToString(await resources.getRawFileContent('rime/manifest.json')));
    for (const name of files) {
      if (name.includes('/') || name.includes('..')) throw new Error('Rime 资源名称无效');
      const path = shared + '/' + name;
      if (fs.accessSync(path)) continue;
      const bytes = await resources.getRawFileContent('rime/' + name);
      const file = await fs.open(path + '.tmp', fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC);
      try {
        let offset = 0;
        while (offset < bytes.byteLength) {
          const chunk = bytes.buffer.slice(bytes.byteOffset + offset,
            bytes.byteOffset + Math.min(bytes.byteLength, offset + 262144));
          const written = await fs.write(file.fd, chunk);
          if (written <= 0 || written > chunk.byteLength) throw new Error('Rime 资源写入不完整');
          offset += written;
        }
      } finally { await fs.close(file); }
      await fs.rename(path + '.tmp', path);
    }
    await rimeRequest('initialize', shared, filesDir + '/' + profile);
    console.info('[OpenLess Rime] initialized');
  })().catch((error: Error) => { initialization = undefined; throw error; });
  return initialization;
}

export async function testRime(filesDir: string, resources: resourceManager.ResourceManager): Promise<boolean> {
  if (validated) return true;
  // The private schema used below cannot learn. Keep initialization on the
  // real profile so a main-window probe cannot redirect future IME sessions.
  await initializeRime(filesDir, resources);
  let state = await rimeRequest('create', '', 'private');
  const id = state.session;
  try {
    for (const key of 'nihao') state = await rimeRequest('key', id, key.charCodeAt(0).toString());
    if (!state.candidates.includes('你好')) throw new Error('Rime 中文候选自检失败');
    const revision = state.revision;
    state = await rimeRequest('key', id, '65288');
    if (state.input !== 'niha') throw new Error('Rime 编码退格自检失败');
    let rejected: boolean = false;
    try { await rimeRequest('select', id, '0@' + revision.toString()); } catch (_) { rejected = true; }
    if (!rejected) throw new Error('Rime 过期候选自检失败');
    state = await rimeRequest('key', id, '111');
    state = await rimeRequest('select', id, state.candidates.indexOf('你好').toString() + '@' + state.revision.toString());
    if (state.commit !== '你好') throw new Error('Rime 中文确认自检失败');
    await rimeRequest('ack', id);
    state = await rimeRequest('clear', id);
    if (state.input || state.commit) throw new Error('Rime 取消自检失败');
    for (const key of 'nihao') state = await rimeRequest('key', id, key.charCodeAt(0).toString());
    state = await rimeRequest('key', id, '32');
    if (state.commit !== '你好') throw new Error('Rime 空格确认自检失败');
    await rimeRequest('ack', id);
    console.info('[OpenLess Rime] candidate/backspace/commit/cancel/stale/space test passed');
    validated = true;
    return true;
  } finally { await rimeRequest('destroy', id); }
}
