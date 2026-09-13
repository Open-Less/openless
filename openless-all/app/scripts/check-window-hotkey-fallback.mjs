import * as esbuild from 'esbuild';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const tmp = await mkdtemp(join(tmpdir(), 'openless-window-hotkey-fallback-'));
const outfile = join(tmp, 'window-hotkey-fallback-test.mjs');

try {
  await esbuild.build({
    entryPoints: [
      fileURLToPath(new URL('../src/lib/windowHotkeyFallback.test.ts', import.meta.url)),
    ],
    outfile,
    bundle: true,
    platform: 'node',
    format: 'esm',
    logLevel: 'silent',
  });
  await import(pathToFileURL(outfile).href);
} finally {
  await rm(tmp, { recursive: true, force: true });
}
