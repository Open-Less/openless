#!/usr/bin/env node
// Build-time tool only. Installed users need neither Node/npm nor Cargo.
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { access, chmod, copyFile, cp, mkdir, readFile, readdir, rename, rm, stat, writeFile } from 'node:fs/promises';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const APP_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const NODE_VERSION = '22.23.2';
// Pinned from https://nodejs.org/dist/v22.23.2/SHASUMS256.txt.
// Never trust a freshly downloaded checksum to authorize a different binary.
export const NODE_SHA256 = Object.freeze({
  'darwin-arm64': '61130f394c1630d211dd50aecc4353d379480f36d3ac913cd85dbba1aed585c6',
  'darwin-x64': '58e99022c2ff89395576cc7fd4d98cea24bb68081475d5f88b801ee8729fb026',
  'linux-arm64': '013b59cfd2819703a6f4a14ab891fc46fc2a4e3f5bcd92de3fb4929b43e35b30',
  'linux-x64': 'b294a556e639d64338823920e5866c21c02741742d2e1529ee1a225c1ec9252a',
  'win-arm64': 'fec025a6da31757e3b6af84c5a1628e9d38442ca99a2161091d78f2fcfa35ef3',
  'win-x64': '1177b4137ba5adaa56354ae40f1080c7450e8ae09cecb47da459d1c52ac99f97',
});

export function resolveTarget(target, platform = process.platform, arch = process.arch) {
  if (target?.includes('android') || target?.includes('ios')) return null;
  const targets = {
    'aarch64-apple-darwin': ['darwin', 'arm64'],
    'x86_64-apple-darwin': ['darwin', 'x64'],
    'aarch64-pc-windows-msvc': ['win', 'arm64'],
    'x86_64-pc-windows-msvc': ['win', 'x64'],
    'x86_64-pc-windows-gnu': ['win', 'x64'],
    'aarch64-unknown-linux-gnu': ['linux', 'arm64'],
    'x86_64-unknown-linux-gnu': ['linux', 'x64'],
  };
  const hostPlatform = platform === 'win32' ? 'win' : platform;
  const pair = target ? targets[target] : [hostPlatform, arch];
  if (!pair || !NODE_SHA256[pair.join('-')]) {
    throw new Error(`不支持的 PI 桌面构建目标：${target || `${platform}/${arch}`}`);
  }
  if (pair[0] !== hostPlatform || pair[1] !== arch) {
    throw new Error(`PI 安装包需要在对应平台/架构构建以安装正确的原生依赖：${pair.join('-')}`);
  }
  const id = pair.join('-');
  return {
    id,
    target: target || Object.keys(targets).find((key) => targets[key].join('-') === id),
    archive: `node-v${NODE_VERSION}-${id}.${pair[0] === 'win' ? 'zip' : 'tar.gz'}`,
    nodeName: pair[0] === 'win' ? 'node.exe' : 'node',
    computerName: pair[0] === 'win' ? 'openless-computer.exe' : 'openless-computer',
  };
}

export function assertWithin(root, path) {
  const delta = relative(resolve(root), resolve(path));
  if (!delta || delta === '..' || delta.startsWith(`..${sep}`) || delta.includes(':') || resolve(root) === resolve(path)) {
    throw new Error(`拒绝修改构建目录以外的路径：${path}`);
  }
  return path;
}

async function exists(path) {
  try { await access(path); return true; } catch { return false; }
}

export function verifyDigest(buffer, expected, label = 'Node archive') {
  const actual = createHash('sha256').update(buffer).digest('hex');
  if (actual !== expected) throw new Error(`${label} SHA-256 校验失败：${actual}`);
  return actual;
}

export async function pruneDevelopmentFiles(directory) {
  let removed = 0;
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = assertWithin(directory, join(directory, entry.name));
    if (entry.isDirectory()) {
      removed += await pruneDevelopmentFiles(path);
    } else if (entry.isFile() && /(?:\.d\.(?:ts|mts|cts)|\.(?:js|mjs|cjs|ts|mts|cts)\.map)$/.test(entry.name)) {
      // Node executes JavaScript/native modules; declarations and source maps
      // can exceed NSIS MAX_PATH in otherwise ordinary Windows checkouts.
      await rm(path);
      removed += 1;
    }
  }
  return removed;
}

async function validateWindowsBundlePaths(staging, output) {
  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await visit(path);
      else if (join(output, relative(staging, path)).length >= 260) {
        throw new Error(`NSIS 资源路径超过 Windows 长度限制，请将源码移至更短的目录：${relative(staging, path)}`);
      }
    }
  }
  await visit(staging);
}

function run(executable, args, options = {}) {
  const result = spawnSync(executable, args, {
    cwd: APP_ROOT, stdio: 'inherit', windowsHide: true, ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${executable} 执行失败，退出码 ${result.status}`);
  return result;
}

async function sourceFingerprint(target) {
  const hash = createHash('sha256').update(target.id).update(NODE_VERSION);
  async function visit(path) {
    const metadata = await stat(path);
    if (metadata.isDirectory()) {
      for (const name of (await readdir(path)).sort()) {
        if (!['node_modules', 'target', 'test', 'tests', '.git'].includes(name)) await visit(join(path, name));
      }
    } else {
      hash.update(relative(APP_ROOT, path));
      hash.update(await readFile(path));
    }
  }
  for (const path of ['pi-backend', 'crates/openless-computer', 'Cargo.toml', 'Cargo.lock', 'scripts/prepare-pi-backend.mjs', 'scripts/pi-node-entitlements.plist']) {
    await visit(join(APP_ROOT, path));
  }
  return hash.digest('hex');
}

async function downloadNode(target, cache) {
  const archive = assertWithin(cache, join(cache, target.archive));
  if (!await exists(archive)) {
    console.log(`[pi] 下载 Node ${NODE_VERSION} (${target.id})`);
    const response = await fetch(`https://nodejs.org/dist/v${NODE_VERSION}/${target.archive}`, {
      signal: AbortSignal.timeout(180_000),
    });
    if (!response.ok || !response.body) throw new Error(`Node 下载失败：HTTP ${response.status}`);
    const partial = `${archive}.partial`;
    try {
      await pipeline(Readable.fromWeb(response.body), createWriteStream(partial));
      verifyDigest(await readFile(partial), NODE_SHA256[target.id], target.archive);
      await rename(partial, archive);
    } finally {
      await rm(partial, { force: true });
    }
  }
  verifyDigest(await readFile(archive), NODE_SHA256[target.id], target.archive);
  const extracted = assertWithin(cache, join(cache, `node-v${NODE_VERSION}-${target.id}`));
  await rm(extracted, { recursive: true, force: true });
  run('tar', ['-xf', archive, '-C', cache]);
  return extracted;
}

async function npmCli() {
  const candidates = [
    process.env.npm_execpath,
    join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'),
    join(dirname(process.execPath), '../lib/node_modules/npm/bin/npm-cli.js'),
  ].filter(Boolean);
  for (const path of candidates) if (await exists(path)) return resolve(path);
  throw new Error('构建环境缺少 npm CLI；请通过 npm 执行准备脚本，或安装 Node.js 开发工具链。');
}

async function signMacPayload(directory) {
  if (process.platform !== 'darwin') return;
  const identity = process.env.APPLE_SIGNING_IDENTITY || '-';
  const executables = [];
  async function visit(path) {
    const metadata = await stat(path);
    if (metadata.isDirectory()) {
      for (const name of await readdir(path)) await visit(join(path, name));
    } else if (metadata.mode & 0o111 || path.endsWith('.node') || path.endsWith('.dylib')) {
      const header = (await readFile(path)).subarray(0, 4).toString('hex');
      if (['cffaedfe', 'cefaedfe', 'feedfacf', 'feedface', 'cafebabe', 'bebafeca'].includes(header)) executables.push(path);
    }
  }
  await visit(directory);
  for (const executable of executables) {
    const args = ['--force', '--sign', identity];
    if (identity !== '-') args.push('--options', 'runtime', '--timestamp');
    if (executable === join(directory, 'node')) args.push('--entitlements', join(APP_ROOT, 'scripts/pi-node-entitlements.plist'));
    run('codesign', [...args, executable]);
  }
}

export async function prepare(argv = process.argv.slice(2)) {
  if (argv.includes('--help')) {
    console.log('node scripts/prepare-pi-backend.mjs [--target RUST_TARGET] [--force]\n在本机平台构建并缓存完整 PI + Node + Computer 安装资源。');
    return;
  }
  const targetFlag = argv.indexOf('--target');
  if (targetFlag !== -1 && !argv[targetFlag + 1]) throw new Error('--target 缺少 Rust target');
  let requested = targetFlag === -1
    ? process.env.TAURI_ENV_TARGET_TRIPLE || process.env.CARGO_BUILD_TARGET
    : argv[targetFlag + 1];
  if (!requested) {
    const rustc = run('rustc', ['-vV'], { stdio: 'pipe', encoding: 'utf8' });
    requested = rustc.stdout.match(/^host: (.+)$/m)?.[1]?.trim();
  }
  const target = resolveTarget(requested);
  if (!target || process.env.TAURI_ENV_PLATFORM === 'android' || process.env.TAURI_ENV_PLATFORM === 'ios') {
    console.log('[pi] 移动端不包含桌面 Computer 后端');
    return;
  }
  const cache = join(APP_ROOT, '.cache/pi-backend');
  const output = join(APP_ROOT, 'src-tauri/resources/pi-backend');
  await mkdir(cache, { recursive: true });
  const fingerprint = await sourceFingerprint(target);
  let previous;
  try { previous = JSON.parse(await readFile(join(output, 'manifest.json'), 'utf8')); } catch { /* fresh build */ }
  if (!argv.includes('--force') && previous?.fingerprint === fingerprint &&
      await exists(join(output, target.nodeName)) && await exists(join(output, target.computerName)) &&
      await exists(join(output, 'runtime/index.mjs')) && await exists(join(output, 'runtime/node_modules'))) {
    await signMacPayload(output);
    console.log(`[pi] 使用已准备的 ${target.id} 安装资源`);
    return;
  }
  const extracted = await downloadNode(target, cache);
  const staging = assertWithin(cache, join(cache, `staging-${process.pid}`));
  await rm(staging, { recursive: true, force: true });
  await mkdir(staging, { recursive: true });
  try {
    const source = join(APP_ROOT, 'pi-backend');
    const runtime = join(staging, 'runtime');
    await cp(source, runtime, {
      recursive: true,
      filter: (path) => !relative(source, path).split(sep).some((name) => ['node_modules', 'test', 'tests', '.git'].includes(name)),
    });
    const node = join(extracted, target.id.startsWith('win-') ? 'node.exe' : 'bin/node');
    await copyFile(node, join(staging, target.nodeName));
    await copyFile(join(extracted, 'LICENSE'), join(staging, 'NODE-LICENSE'));
    await chmod(join(staging, target.nodeName), 0o755);
    run(process.execPath, [await npmCli(), 'ci', '--ignore-scripts', '--omit=dev', '--no-audit', '--no-fund'], { cwd: runtime });
    const pruned = await pruneDevelopmentFiles(join(runtime, 'node_modules'));
    console.log(`[pi] 移除 ${pruned} 个运行时无需使用的类型声明与源码映射`);
    console.log('[pi] 编译原生 Computer 后端');
    run('cargo', ['build', '--locked', '--release', '-p', 'openless-computer', '--target', target.target]);
    const cargoTarget = resolve(APP_ROOT, process.env.CARGO_TARGET_DIR || 'target');
    await copyFile(join(cargoTarget, target.target, 'release', target.computerName), join(staging, target.computerName));
    await chmod(join(staging, target.computerName), 0o755);
    await signMacPayload(staging);
    run(join(staging, target.nodeName), [join(runtime, 'index.mjs'), '--health'], {
      env: { ...process.env, OPENLESS_COMPUTER_BIN: join(staging, target.computerName) },
    });
    run(join(staging, target.computerName), ['--capabilities']);
    await writeFile(join(staging, 'manifest.json'), `${JSON.stringify({
      format: 1, target: target.target, node: NODE_VERSION, fingerprint,
    }, null, 2)}\n`, 'utf8');
    if (target.id.startsWith('win-')) await validateWindowsBundlePaths(staging, output);
    await mkdir(dirname(output), { recursive: true });
    assertWithin(join(APP_ROOT, 'src-tauri/resources'), output);
    await rm(output, { recursive: true, force: true });
    await rename(staging, output);
    console.log(`[pi] 完整安装资源已就绪：${output}`);
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  prepare().catch((error) => { console.error(`[pi] ${error.message}`); process.exitCode = 1; });
}
