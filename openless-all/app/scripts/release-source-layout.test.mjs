import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { load } from 'js-yaml';

const workflows = ['release-tauri.yml', 'release-harmony.yml'].map((name) =>
  load(readFileSync(new URL(`../../../.github/workflows/${name}`, import.meta.url), 'utf8')),
);
const scratch = mkdtempSync(join(tmpdir(), 'openless-release-layout-'));
let bashCount = 0;
let pwshCount = 0;
try {
  for (const workflow of workflows) {
    for (const job of Object.values(workflow.jobs)) {
      for (const step of job.steps ?? []) {
        const run = step.run ?? '';
        // Execute the actual resolver, not a reimplementation, in both source
        // layouts. The both-present case proves new paths take precedence.
        for (const match of run.matchAll(
          /if \[ -f ([^\s]+) \]; then\n\s+(\w+)=\1\n\s*elif \[ -f ([^\s]+) \]; then\n\s+\2=\3\n\s*else\n\s+echo "::error::Missing source input:[^\n]+\n\s+exit 1\n\s*fi/g,
        )) {
          const [block, current, variable, legacy] = match;
          bashCount++;
          for (const mode of ['new', 'old', 'both', 'missing']) {
            const root = join(scratch, `bash-${bashCount}-${mode}`);
            const cwd = join(root, 'app');
            mkdirSync(cwd, { recursive: true });
            for (const [path, layout] of [
              [current, 'new'],
              [legacy, 'old'],
            ]) {
              if (mode !== layout && mode !== 'both') continue;
              const destination = join(cwd, path);
              mkdirSync(dirname(destination), { recursive: true });
              // Revision and operational scripts are both selected as files.
              writeFileSync(destination, layout + '\n');
            }
            const result = spawnSync('bash', ['-eu', '-c', block + `\ncat "$${variable}"`], {
              cwd,
              encoding: 'utf8',
              timeout: 5000,
            });
            assert.ifError(result.error);
            if (mode === 'missing') {
              assert.notEqual(result.status, 0);
              assert.match(result.stderr, /Missing source input/);
            } else {
              assert.equal(result.status, 0, result.stderr);
              assert.equal(result.stdout.trim(), mode === 'old' ? 'old' : 'new');
            }
          }
          // A resolver is useful only if the actual invocation consumes it.
          assert.match(run, new RegExp(`(?:bash|node|swift|python3|cat) "\\$${variable}"`));
        }
        // PowerShell is not available on every contract host. Assert its exact
        // guarded structure and run the corresponding selection on real files;
        // this is not a claim that the PowerShell runtime ran.
        for (const match of run.matchAll(
          /if \(Test-Path -LiteralPath '([^']+)' -PathType Leaf\) \{\n\s+\$(\w+) = '\1'\n\s*\} elseif \(Test-Path -LiteralPath '([^']+)' -PathType Leaf\) \{\n\s+\$\2 = '\3'\n\s*\} else \{\n\s+throw "Missing source input:[^\n]+\n\s*\}/g,
        )) {
          const [, current, variable, legacy] = match;
          pwshCount++;
          assert.ok(run.includes(`& $${variable}`));
          for (const mode of ['new', 'old', 'both', 'missing']) {
            const cwd = join(scratch, `pwsh-${pwshCount}-${mode}`, 'app');
            mkdirSync(cwd, { recursive: true });
            for (const [path, layout] of [
              [current, 'new'],
              [legacy, 'old'],
            ]) {
              if (mode !== layout && mode !== 'both') continue;
              mkdirSync(dirname(join(cwd, path)), { recursive: true });
              writeFileSync(join(cwd, path), layout);
            }
            const resolve = () => {
              for (const path of [current, legacy]) {
                if (existsSync(join(cwd, path))) return readFileSync(join(cwd, path), 'utf8');
              }
              throw new Error('Missing source input');
            };
            if (mode === 'missing') assert.throws(resolve, /Missing source input/);
            else assert.equal(resolve(), mode === 'old' ? 'old' : 'new');
          }
        }
      }
    }
  }
  assert.equal(
    bashCount,
    10,
    'four macOS scripts and six Harmony inputs require source-tree fallback',
  );
  assert.equal(pwshCount, 2, 'both Windows source build scripts require source-tree fallback');
  const tauri = readFileSync(
    new URL('../../../.github/workflows/release-tauri.yml', import.meta.url),
    'utf8',
  );
  for (const script of ['windows-ime-install-smoke.ps1', 'windows-symbol-pair.py']) {
    assert.ok(tauri.includes(`$scriptPath = 'openless-all/windows/scripts/${script}'`));
    assert.ok(
      tauri
        .replaceAll('\\', '/')
        .includes(`release-tooling/openless-all/windows/scripts/${script}`),
    );
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log(
  'Release source layout: 10 real Bash resolvers and 2 PowerShell contract fixtures passed',
);
