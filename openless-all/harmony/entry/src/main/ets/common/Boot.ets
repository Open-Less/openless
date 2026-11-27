/**
 * Launch parameters, before any UI exists.
 *
 * `AppStorage` is not safe to touch from `UIAbility.onWindowStageCreate` (the UI
 * instance owns it), so the ability records the deep link here and `Index` picks
 * it up in `aboutToAppear`. `aa start --ps route history --ps layout sm` is the
 * supported launcher/QA form; unknown values fall back to the overview.
 */
export class Boot {
  static route: string = '';
  static layout: string = '';

  static takeRoute(): string {
    const value: string = Boot.route;
    Boot.route = '';
    return value;
  }

  static takeLayout(): string {
    const value: string = Boot.layout;
    Boot.layout = '';
    return value;
  }
}
