import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';

const script = readFileSync(
  new URL('../../windows/scripts/windows-build-gnu.ps1', import.meta.url),
  'utf8',
);
for (const pattern of [
  /git -c core\.quotePath=false ls-files --cached --others --exclude-standard/,
  /IsNullOrWhiteSpace\(\$_\)/,
  /Test-Path -LiteralPath \$src -PathType Leaf/,
  /\.openless-mirror-manifest\.txt/,
  /\[IO\.File\]::ReadAllLines\(\$manifestPath, \[Text\.Encoding\]::UTF8\)/,
  /\[Collections\.Generic\.HashSet\[string\]\]::new\(\[string\[\]\]\$inputs, \[StringComparer\]::OrdinalIgnoreCase\)/,
  /\$inputSet\.Contains\(\$previous\)/,
  /Resolve-MirrorInput \$previous/,
  /Remove-Item -LiteralPath \$stale -Force/,
  /Get-ChildItem -LiteralPath \$parent -Force/,
  /\$destination\.StartsWith\(\$mirrorPrefix, \[StringComparison\]::OrdinalIgnoreCase\)/,
  /ReparsePoint/,
  /\$normalized = \$relative -replace '\\\\', '\/'/,
  /\[IO\.File\]::WriteAllLines\(\$manifestPath, \[string\[\]\]\$inputs, \[Text\.UTF8Encoding\]::new\(\$false\)\)/,
  /Join-Path \$MirrorRoot "app"/,
])
  assert.match(script, pattern);
assert.ok(
  script.indexOf('Remove-Item -LiteralPath $stale') < script.indexOf('Copy-Item -LiteralPath'),
);
assert.ok(script.indexOf('Copy-Item -LiteralPath') < script.indexOf('[IO.File]::WriteAllLines'));

const guard = script.match(/\$normalized -match '([^']+)'/);
assert.ok(guard, 'protected-cache regex must be extracted from the PowerShell guard');
const protectedCache = new RegExp(guard[1], 'i');
const normalize = (path) => path.replaceAll('\\', '/');
const repositoryInputs = execFileSync(
  'git',
  [
    '-c',
    'core.quotePath=false',
    'ls-files',
    '--cached',
    '--others',
    '--exclude-standard',
    '--',
    '.',
  ],
  { cwd: new URL('../..', import.meta.url), encoding: 'utf8' },
)
  .split(/\r?\n/)
  .filter(Boolean);
const targetSource = 'app/src-tauri/src/ask_window/target/macos.rs';
assert.ok(
  repositoryInputs.includes(targetSource),
  'real git inputs must include the target module',
);
for (const path of repositoryInputs) {
  for (const variant of [path, path.replaceAll('/', '\\')]) {
    assert.doesNotMatch(normalize(variant), protectedCache, `repository input: ${variant}`);
  }
}
const protectedCaches = [
  'node_modules/cached.js',
  'target/cache',
  'dist/cache',
  '.artifacts/cache',
  'app/node_modules/cached.js',
  'app/src/nested/node_modules/cached.js',
  'app/target/cache',
  'app/src-tauri/target/cache',
  'app/src-tauri/backend-tests/target/cache',
  'app/dist/cache',
  'app/.artifacts/cache',
  'linux/egui/target/cache',
  'harmony/rust/target/cache',
  'ios/CoreHost/target/cache',
];
for (const path of protectedCaches) {
  for (const variant of [path, path.replaceAll('/', '\\')]) {
    assert.match(normalize(variant), protectedCache, `protected cache: ${variant}`);
  }
}

// Filesystem fixture of the manifest algorithm; PowerShell execution belongs
// to the Windows runner. Only manifest-owned files may be removed.
const scratch = mkdtempSync(join(tmpdir(), 'openless-gnu-mirror-'));
const source = join(scratch, 'source with spaces');
const mirror = join(scratch, 'mirror');
const manifest = join(mirror, '.openless-mirror-manifest.txt');
const put = (root, path, text = path) => {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), text);
};
const destination = (path) => {
  const normalized = normalize(path);
  const target = resolve(mirror, normalized);
  assert.ok(target.startsWith(mirror + sep), 'input must remain inside mirror');
  assert.doesNotMatch(normalized, protectedCache);
  for (let ancestor = target; ancestor !== mirror; ancestor = dirname(ancestor)) {
    if (existsSync(ancestor)) assert.equal(lstatSync(ancestor).isSymbolicLink(), false);
  }
  return target;
};
const sync = (enumerated) => {
  const inputs = [
    ...new Set(
      enumerated.filter(
        (path) =>
          path.trim() && existsSync(join(source, path)) && lstatSync(join(source, path)).isFile(),
      ),
    ),
  ].sort();
  const inputSet = new Set(inputs.map((path) => path.toLowerCase()));
  const previous = existsSync(manifest)
    ? readFileSync(manifest, 'utf8').split('\n').filter(Boolean)
    : [];
  for (const path of previous) {
    if (inputSet.has(path.toLowerCase())) continue;
    const target = destination(path);
    if (existsSync(target) && lstatSync(target).isFile()) rmSync(target);
    for (
      let parent = dirname(target);
      parent !== mirror && existsSync(parent);
      parent = dirname(parent)
    ) {
      if (readdirSync(parent).length) break;
      rmdirSync(parent);
    }
  }
  for (const path of inputs) {
    const target = destination(path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(source, path), target);
  }
  writeFileSync(manifest, inputs.join('\n') + '\n', 'utf8');
  return inputs;
};
try {
  const original = [
    'app/public/retired/old.png',
    'app/src/old.ts',
    'app/src/空 格.ts',
    'windows/ime/input.cpp',
    'linux/egui/Cargo.toml',
    'harmony/rust/Cargo.toml',
    targetSource,
  ];
  for (const path of original) put(source, path);
  assert.deepEqual(sync([...original, '', '   ']), [...original].sort());
  for (const path of original) assert.equal(readFileSync(join(mirror, path), 'utf8'), path);
  assert.notEqual(readFileSync(manifest)[0], 0xef, 'manifest is UTF-8 without BOM');
  for (const path of protectedCaches) {
    put(mirror, path, 'keep');
  }
  rmSync(join(source, original[0]));
  rmSync(join(source, original[1]));
  put(source, 'app/src/renamed.ts', 'renamed');
  // Git can still enumerate both removed tracked inputs before staging.
  sync([...original, 'app/src/renamed.ts']);
  assert.equal(existsSync(join(mirror, original[0])), false);
  assert.equal(existsSync(join(mirror, 'app/public/retired')), false);
  assert.equal(existsSync(join(mirror, original[1])), false);
  assert.equal(readFileSync(join(mirror, 'app/src/renamed.ts'), 'utf8'), 'renamed');
  assert.equal(readFileSync(join(mirror, targetSource), 'utf8'), targetSource);
  for (const path of protectedCaches) {
    assert.equal(readFileSync(join(mirror, path), 'utf8'), 'keep');
  }
  sync([...original, 'app/src/renamed.ts']); // reuse remains idempotent
  assert.equal(readFileSync(join(mirror, targetSource), 'utf8'), targetSource);
  rmSync(join(source, targetSource));
  sync([...original, 'app/src/renamed.ts']);
  assert.equal(existsSync(join(mirror, targetSource)), false);
  assert.equal(existsSync(dirname(join(mirror, targetSource))), false);
  for (const invalid of ['../outside', ...protectedCaches]) {
    for (const variant of [invalid, invalid.replaceAll('/', '\\')]) {
      assert.throws(() => destination(variant));
      writeFileSync(manifest, variant + '\n');
      assert.throws(() => sync([]));
    }
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log(
  'Windows GNU mirror contracts and manifest deletion fixtures passed (PowerShell runtime not executed)',
);
