import { openAndroidLocalMigration } from './local-migration';

if (typeof window !== 'undefined') {
  throw new Error('local migration IPC mock test must run without a browser window');
}

const result = await openAndroidLocalMigration();
if (result !== undefined) {
  throw new Error('Android local migration mock should resolve without a native result');
}

console.log('local-migration IPC mock contract passed');
