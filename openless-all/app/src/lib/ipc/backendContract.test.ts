import { validateStartupSnapshot } from './shared';

validateStartupSnapshot({ contractVersion: '2.0.0', backend: { running: true } });

for (const snapshot of [
  { contractVersion: '1.0.0', backend: { running: true } },
  { contractVersion: '2.0.0', backend: { running: false } },
]) {
  let failed = false;
  try {
    validateStartupSnapshot(snapshot);
  } catch {
    failed = true;
  }
  if (!failed) throw new Error(`invalid startup snapshot was accepted: ${JSON.stringify(snapshot)}`);
}

console.log('backendContract.test.ts passed');
