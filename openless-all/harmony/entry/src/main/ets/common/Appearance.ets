import { common, ConfigurationConstant } from '@kit.AbilityKit';

/**
 * System appearance.
 *
 * The v1 settings page exposed a 跟随系统 / 浅色 / 深色 switch that wrote a
 * preference nobody read: the app always followed the system. v2 applies the
 * preference through `ApplicationContext.setColorMode`, so the switch actually
 * works and both the main pages and the IME panel resources follow it.
 */
export class Appearance {
  static apply(context: common.UIAbilityContext, theme: string): void {
    try {
      const app = context.getApplicationContext();
      if (theme === 'light') {
        app.setColorMode(ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT);
      } else if (theme === 'dark') {
        app.setColorMode(ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
      } else {
        app.setColorMode(ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET);
      }
    } catch (error) {
      console.warn('[OpenLess Appearance] color mode not applied');
    }
  }
}
