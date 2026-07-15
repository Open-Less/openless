import { readdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, relative, resolve, sep } from 'node:path';
import process from 'node:process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const DEFAULT_APP_ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

function collectFiles(directory, predicate, files = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const entryPath = join(directory, entry.name);
    if (entry.isDirectory()) {
      collectFiles(entryPath, predicate, files);
    } else if (entry.isFile() && predicate(entry.name)) {
      files.push(entryPath);
    }
  }
  return files;
}

export function discoverTestFiles(appRoot = DEFAULT_APP_ROOT) {
  return [
    ...collectFiles(join(appRoot, 'src'), (name) => name.endsWith('.test.ts')),
    ...collectFiles(
      join(appRoot, 'scripts'),
      (name) => name.endsWith('.test.mjs') || (name.startsWith('check-') && name.endsWith('.mjs')),
    ),
  ]
    .map((file) => relative(appRoot, file).split(sep).join('/'))
    .sort();
}

export function runTestFiles(
  testFiles,
  {
    appRoot = DEFAULT_APP_ROOT,
    log = console.log,
    spawn = spawnSync,
    tsxCli,
  } = {},
) {
  let resolvedTsxCli = tsxCli;

  for (const testFile of testFiles) {
    const absoluteTestFile = resolve(appRoot, testFile);
    let args;
    if (testFile.endsWith('.test.ts')) {
      resolvedTsxCli ??= fileURLToPath(import.meta.resolve('tsx/cli'));
      args = [resolvedTsxCli, absoluteTestFile];
    } else if (testFile.endsWith('.mjs')) {
      args = [absoluteTestFile];
    } else {
      log(`[frontend-tests] unsupported test file: ${testFile}`);
      return 1;
    }

    log(`[frontend-tests] ${testFile}`);
    const result = spawn(process.execPath, args, {
      cwd: appRoot,
      env: process.env,
      stdio: 'inherit',
    });
    if (result.error) {
      log(`[frontend-tests] failed to start ${testFile}: ${result.error.message}`);
      return 1;
    }
    if (result.status !== 0) {
      return result.status ?? 1;
    }
  }

  return 0;
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : '';
if (invokedPath === import.meta.url) {
  const testFiles = discoverTestFiles();
  if (testFiles.length === 0) {
    console.error('[frontend-tests] no tests discovered');
    process.exitCode = 1;
  } else {
    console.log(`[frontend-tests] discovered ${testFiles.length} tests`);
    process.exitCode = runTestFiles(testFiles);
  }
}
