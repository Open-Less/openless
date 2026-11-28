import { window } from '@kit.ArkUI';

/**
 * Launch handoff before the page exists. The ability owns the main window;
 * pages must not select the last window, which may be a sheet or dialog.
 * Route hints are consumed once. Layout always follows the real window.
 */
export class Boot {
  static route: string = '';
  static mainWindow?: window.Window;

  static takeRoute(): string {
    const value: string = Boot.route;
    Boot.route = '';
    return value;
  }
}
