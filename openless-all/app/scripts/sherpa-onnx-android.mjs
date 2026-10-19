/**
 * Fetches the official sherpa-onnx Android libraries that local ASR links on
 * arm64 Android builds.
 *
 * The sherpa-onnx crate downloads prebuilt libraries for desktop targets only;
 * for Android it reads SHERPA_ONNX_LIB_DIR. This pins the release, checks the
 * archive's size and SHA-256, and extracts the arm64-v8a
 * libsherpa-onnx-c-api.so and libonnxruntime.so into a cache under
 * src-tauri/target/, so a build never links an unverified download.
 *
 * Usage (also called by run-android-tauri-build.mjs):
 *   node scripts/sherpa-onnx-android.mjs   # prints the library directory
 */
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream, existsSync, mkdirSync, renameSync, rmSync, statSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

// Must match the sherpa-onnx crate version in src-tauri/Cargo.toml.
export const SHERPA_ONNX_VERSION = '1.13.2';
export const SHERPA_ONNX_ANDROID_ARCHIVE = {
  name: `sherpa-onnx-v${SHERPA_ONNX_VERSION}-android.tar.bz2`,
  url: `https://github.com/k2-fsa/sherpa-onnx/releases/download/v${SHERPA_ONNX_VERSION}/sherpa-onnx-v${SHERPA_ONNX_VERSION}-android.tar.bz2`,
  sizeBytes: 52_236_308,
  sha256: 'fc4d17941152941a883b0cfabfc9acac118682324e9f97df6c1ae1360bc7bc8e',
};
/** The only ABI local ASR is built for; the others stay cloud-only. */
export const SHERPA_ONNX_ANDROID_ABI = 'arm64-v8a';
/** What the app links and ships: libopenless_lib.so needs the C API, which needs onnxruntime. */
export const SHERPA_ONNX_ANDROID_LIBS = ['libsherpa-onnx-c-api.so', 'libonnxruntime.so'];

const appRoot = fileURLToPath(new URL('..', import.meta.url));

export function sherpaOnnxAndroidCacheDir(root = appRoot) {
  return join(root, 'src-tauri', 'target', 'sherpa-onnx-android', `v${SHERPA_ONNX_VERSION}`);
}

export function sherpaOnnxAndroidLibDir(root = appRoot) {
  return join(sherpaOnnxAndroidCacheDir(root), 'jniLibs', SHERPA_ONNX_ANDROID_ABI);
}

async function sha256Of(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

/** Returns the directory holding the verified arm64 libraries, downloading them first if needed. */
export async function ensureSherpaOnnxAndroidLibs({ root = appRoot, log = console.log } = {}) {
  const libDir = sherpaOnnxAndroidLibDir(root);
  if (SHERPA_ONNX_ANDROID_LIBS.every((lib) => existsSync(join(libDir, lib)))) return libDir;

  const cacheDir = sherpaOnnxAndroidCacheDir(root);
  mkdirSync(cacheDir, { recursive: true });
  const archive = join(cacheDir, SHERPA_ONNX_ANDROID_ARCHIVE.name);
  const valid = async () =>
    existsSync(archive) &&
    statSync(archive).size === SHERPA_ONNX_ANDROID_ARCHIVE.sizeBytes &&
    (await sha256Of(archive)) === SHERPA_ONNX_ANDROID_ARCHIVE.sha256;

  if (!(await valid())) {
    log(`[sherpa-onnx] downloading ${SHERPA_ONNX_ANDROID_ARCHIVE.url}`);
    const response = await fetch(SHERPA_ONNX_ANDROID_ARCHIVE.url);
    if (!response.ok) throw new Error(`sherpa-onnx download failed: HTTP ${response.status}`);
    const partial = `${archive}.part`;
    await writeFile(partial, Buffer.from(await response.arrayBuffer()));
    renameSync(partial, archive);
    if (!(await valid())) {
      rmSync(archive, { force: true });
      throw new Error(`sherpa-onnx archive failed size/SHA-256 verification: ${SHERPA_ONNX_ANDROID_ARCHIVE.name}`);
    }
  }

  const members = SHERPA_ONNX_ANDROID_LIBS.map((lib) => `./jniLibs/${SHERPA_ONNX_ANDROID_ABI}/${lib}`);
  const result = spawnSync('tar', ['-xjf', SHERPA_ONNX_ANDROID_ARCHIVE.name, ...members], {
    cwd: cacheDir,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`extracting ${SHERPA_ONNX_ANDROID_ARCHIVE.name} failed (tar exit ${result.status})`);
  for (const lib of SHERPA_ONNX_ANDROID_LIBS) {
    if (!existsSync(join(libDir, lib))) throw new Error(`${lib} missing from ${SHERPA_ONNX_ANDROID_ARCHIVE.name}`);
  }
  log(`[sherpa-onnx] libraries ready in ${libDir}`);
  return libDir;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  ensureSherpaOnnxAndroidLibs({ log: (line) => console.error(line) })
    .then((dir) => console.log(dir))
    .catch((error) => {
      console.error(error.message ?? error);
      process.exit(1);
    });
}
