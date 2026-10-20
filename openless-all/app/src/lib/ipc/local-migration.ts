import { invokeOrMock } from './shared';

/** Opens the native Android SAF migration flow from the application settings. */
export function openAndroidLocalMigration(): Promise<void> {
  return invokeOrMock('open_android_local_migration', undefined, () => undefined);
}
