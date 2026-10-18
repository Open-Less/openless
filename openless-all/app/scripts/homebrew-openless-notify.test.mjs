import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compareVersions } from './homebrew-openless-notify.mjs';
import { installReminder } from './install-homebrew-openless-reminder.mjs';

for (const [available, current, expected] of [
  ['1.3.18', '2.0.0-beta-a.beta.6+build.20261015-be', -1],
  ['2.0.0+build.20261016', '2.0.0-beta-a.beta.6+build.20261015-be', 1],
  ['2.0.0+build.20261016', '2.0.0+build.20261016', 0],
  ['2.0.0+build.20261017', '2.0.0+build.20261016', 1],
  ['2.0.0-Beta.10', '2.0.0-Beta.6', 1],
  ['2.0.1', '2.0.0+build.20991231', 1],
  ['2.0.0-1', '2.0.0-alpha', -1],
  ['latest', '2.0.0', null],
])
  assert.equal(compareVersions(available, current), expected);

const home = mkdtempSync(join(tmpdir(), 'openless-brew-reminder-'));
try {
  writeFileSync(join(home, '.zshrc'), '# existing configuration\n');
  const installed = installReminder(home);
  installReminder(home);
  const startup = readFileSync(join(home, '.zshrc'), 'utf8');
  assert.equal(startup.split('# >>> OpenLess Homebrew update reminders >>>').length, 2);
  assert.ok(startup.startsWith('# existing configuration\n'));
  const hook = join(installed, 'homebrew-openless-hook.zsh');
  if (process.platform === 'darwin') {
    assert.equal(spawnSync('/bin/zsh', ['-n', hook]).status, 0);
    for (const [command, pending] of [
      ['brew update', '1'],
      ['command brew update', '1'],
      ['/opt/homebrew/bin/brew update --quiet', '1'],
      ['env KEY=value brew update', '1'],
      ['echo hello && brew update', '1'],
      ['brew upgrade --cask openless', '1'],
      ['echo "brew update"', '0'],
      ["printf '%s' brew update", '0'],
      ['brew info openless', '0'],
      ['brew --prefix', '0'],
    ]) {
      const result = spawnSync(
        '/bin/zsh',
        [
          '-fc',
          'source "$1"; _openless_brew_preexec "$2" "$2"; print -r -- "$_openless_brew_pending"',
          '_',
          hook,
          command,
        ],
        { encoding: 'utf8' },
      );
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout.trim(), pending, command);
    }
  }
  assert.ok(
    readFileSync(join(installed, 'homebrew-openless-notify.mjs'), 'utf8').includes(
      'checkHomebrewOpenless',
    ),
  );
} finally {
  rmSync(home, { recursive: true, force: true });
}
console.log('homebrew-openless-notify.test.mjs passed');
