// Contract test for scripts/ci-cache-usage.sh.
//
// The prune path deletes caches, so it is the one part of the CI tooling that
// must not be trusted on a hunch. This test runs the script against a stubbed
// `gh` and asserts that only the scopes of closed or merged pull requests are
// deleted, that open pull requests are kept, and that an unreadable pull
// request state is treated as "keep".
import { execFileSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = resolve(dirname(fileURLToPath(import.meta.url)), 'ci-cache-usage.sh');
const scratch = mkdtempSync(join(tmpdir(), 'openless-cache-usage-'));
const binDir = join(scratch, 'bin');
const deleteLog = join(scratch, 'deleted.tsv');
mkdirSync(binDir, { recursive: true });

const fixtures = [
  {
    ref: 'refs/heads/beta',
    key: 'v0-rust-cross-platform-Windows_NT-x64-3619e479-a77d3f83',
    size: 1_622_000_000,
  },
  {
    ref: 'refs/pull/101/merge',
    key: 'v0-rust-cross-platform-Windows_NT-x64-3619e479-aaaaaaaa',
    size: 1_600_000_000,
  },
  {
    ref: 'refs/pull/102/merge',
    key: 'v0-rust-build-linux-egui-Linux-x64-32c982f2-6e438e89',
    size: 1_338_000_000,
  },
  {
    ref: 'refs/pull/103/head',
    key: 'v0-rust-macos-msrv-v1-macos-msrv-Darwin-arm64-832c8724-bbbb',
    size: 715_000_000,
  },
  {
    ref: 'refs/tags/v2.0.0-Beta.3-tauri',
    key: 'v0-rust-macos-release-v1-build-Darwin-x64-ab76de4a-c48940d0',
    size: 759_000_000,
  },
];
writeFileSync(
  join(scratch, 'caches.json'),
  JSON.stringify({
    total_count: fixtures.length,
    actions_caches: fixtures.map(({ ref, key, size }) => ({ ref, key, size_in_bytes: size })),
  }),
);

const pullStates = { 101: 'open', 102: 'closed', 103: 'merged' };

// Stub `gh`: serves the cache listing, answers pull request lookups from the
// fixture above, and records every deletion instead of performing it.
writeFileSync(
  join(binDir, 'gh'),
  `#!/usr/bin/env bash
set -euo pipefail
args="$*"
case "$args" in
  *-X\\ DELETE*)
    printf '%s\\n' "$args" >>"$DELETE_LOG"
    ;;
  *actions/caches?per_page=100*)
    python3 - "$CACHES_JSON" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
for entry in data["actions_caches"]:
    print(f"{entry['ref']}\\t{entry['key']}\\t{entry['size_in_bytes']}")
PY
    ;;
  *repos/*/pulls/*)
    number=\${args##*/pulls/}
    number=\${number%% *}
    case "$number" in
      101) echo open ;;
      102) echo closed ;;
      103) echo merged ;;
      *) exit 1 ;;
    esac
    ;;
  *)
    echo "unexpected gh invocation: $args" >&2
    exit 1
    ;;
esac
`,
);
chmodSync(join(binDir, 'gh'), 0o755);

let failures = 0;
const check = (label, actual, expected) => {
  const ok = actual === expected;
  if (!ok) failures += 1;
  console.log(
    `${ok ? 'ok' : 'FAIL'}  ${label} -> ${JSON.stringify(actual)} (expected ${JSON.stringify(expected)})`,
  );
};

const run = (args, env = {}) => {
  try {
    const stdout = execFileSync('bash', [scriptPath, ...args], {
      encoding: 'utf8',
      env: {
        ...process.env,
        PATH: `${binDir}:${process.env.PATH}`,
        CACHES_JSON: join(scratch, 'caches.json'),
        DELETE_LOG: deleteLog,
        ...env,
      },
    });
    return { code: 0, stdout };
  } catch (error) {
    return { code: error.status ?? 1, stdout: `${error.stdout ?? ''}${error.stderr ?? ''}` };
  }
};

try {
  // Over budget without --prune: reported, nothing deleted, exit 1.
  const report = run(['--repo', 'example/openless', '--limit-gb', '5']);
  check('over budget exits 1', report.code, 1);
  check('over budget reports total', /6\.03 GB used/.test(report.stdout), true);
  check('over budget mentions the limit', /exceeds the 5 GB budget/.test(report.stdout), true);

  // Prune: closed and merged pull requests go, open ones stay.
  const prune = run(['--prune', '--repo', 'example/openless', '--limit-gb', '5']);
  check('prune exits 1 while still over budget', prune.code, 1);
  const deleted = readFileSync(deleteLog, 'utf8').trim().split('\n').filter(Boolean);
  check('prune deleted exactly two scopes', deleted.length, 2);
  check(
    'prune deleted the closed pull request',
    deleted.some((line) => line.includes('ref=refs/pull/102/merge')),
    true,
  );
  check(
    'prune deleted the merged pull request',
    deleted.some((line) => line.includes('ref=refs/pull/103/head')),
    true,
  );
  check(
    'prune kept the open pull request',
    deleted.some((line) => line.includes('ref=refs/pull/101/merge')),
    false,
  );
  check(
    'prune kept the branch scope',
    deleted.some((line) => line.includes('refs/heads/beta')),
    false,
  );
  check(
    'prune kept the tag scope',
    deleted.some((line) => line.includes('refs/tags/')),
    false,
  );
  check('prune announces the kept scope', /keep\s+refs\/pull\/101\/merge/.test(prune.stdout), true);

  // Inside the budget: exit 0.
  const healthy = run(['--repo', 'example/openless', '--limit-gb', '10']);
  check('inside budget exits 0', healthy.code, 0);
  check('inside budget is reported', /inside the 10 GB budget/.test(healthy.stdout), true);

  // Usage errors keep the exit code distinct from the budget signal.
  const badFlag = run(['--nonsense']);
  check('unknown flag exits 2', badFlag.code, 2);
} finally {
  rmSync(scratch, { recursive: true, force: true });
}

if (failures > 0) {
  console.error(`ci-cache-usage contract test failed (${failures})`);
  process.exit(1);
}
console.log('ci-cache-usage contract test passed');
