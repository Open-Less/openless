import { spawnSync } from 'node:child_process';

const result = spawnSync(
  'cargo',
  [
    'test',
    '--locked',
    '-p',
    'openless-core',
    'shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics',
    '--',
    '--nocapture',
  ],
  {
    env: process.env,
    encoding: 'utf8',
  },
);

const output = `${result.stdout ?? ''}${result.stderr ?? ''}`;
for (const chunk of (result.stdout ?? '').match(/[\s\S]{1,8192}/g) ?? []) process.stdout.write(chunk);
for (const chunk of (result.stderr ?? '').match(/[\s\S]{1,8192}/g) ?? []) process.stderr.write(chunk);

if (result.status !== 0) {
  if (result.error) console.error(result.error);
  process.exit(result.status ?? 1);
}

if (!output.includes('test api::tests::shared_hotkey_edges_own_hold_auto_and_combo_abort_semantics ... ok')) {
  console.error('Core hotkey edge gate did not execute the expected test.');
  process.exit(1);
}

console.log('Core hotkey edge gate passed.');
