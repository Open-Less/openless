import { invokeOrPreview } from '../runtime/transport';
export interface LinuxInputPluginStatus {
  installed: boolean;
  daemonRunning: boolean;
  loaded: boolean;
  insertionReady: boolean;
  hotkeysReady: boolean;
  metadataReady: boolean;
}
/** Diagnostics only: must never install, start, activate, switch or reload an IME. */
export const getLinuxInputPluginStatus = () => invokeOrPreview<LinuxInputPluginStatus>(
  'get_linux_input_plugin_status', undefined,
  () => ({ installed: false, daemonRunning: false, loaded: false, insertionReady: false, hotkeysReady: false, metadataReady: false }),
);
export * from '../lib/ipc/permissions';
export {
  getAndroidOverlayStatus,
  requestAndroidOverlayPermission,
  showAndroidOverlay,
  hideAndroidOverlay,
  getAndroidAccessibilityStatus,
  requestAndroidAccessibilityPermission,
} from '../lib/ipc/platform-exports';
