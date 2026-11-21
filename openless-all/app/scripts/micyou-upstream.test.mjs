import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const appRoot = fileURLToPath(new URL('..', import.meta.url));
const python = process.platform === 'win32' ? 'python' : 'python3';
for (const args of [['scripts/test_sync_micyou.py'], ['scripts/sync-micyou.py', '--verify']]) {
  const result = spawnSync(python, args, {
    cwd: appRoot,
    env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' },
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
console.log('MicYou sync importer and pinned source checks passed');
