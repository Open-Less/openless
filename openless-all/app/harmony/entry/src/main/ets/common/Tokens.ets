/**
 * Design tokens for the native HarmonyOS UI.
 *
 * Colors are app resources (light + dark in `resources/base|dark/element/color.json`)
 * rather than `sys.color.*`: the palette is the OpenLess brand plus the HarmonyOS
 * neutral ramp, and app resources keep both themes in one place. Layout, radius,
 * type and motion follow the HarmonyOS design language (8vp grid, 12/16vp card
 * radius, 4vp touch rhythm) so the pages read as system pages on phone, tablet
 * and 2-in-1.
 */

/** Color roles. Every one resolves through the resource manager, so dark mode is automatic. */
export class C {
  static bg(): Resource {
    return $r('app.color.ol_bg');
  }

  /** Opaque surface: sheets, inputs, anything that must stay readable. */
  static surface(): Resource {
    return $r('app.color.ol_surface');
  }

  /**
   * Translucent glass for cards and navigation chrome. Pair with
   * `backgroundBlurStyle` so the ambient glow behind it shows through - that is
   * the HarmonyOS "immersive material" look rather than a flat painted card.
   */
  static glass(): Resource {
    return $r('app.color.ol_glass');
  }

  static glassStrong(): Resource {
    return $r('app.color.ol_glass_strong');
  }

  static glassSunken(): Resource {
    return $r('app.color.ol_glass_sunken');
  }

  /** Ambient light sources painted behind the whole app. */
  static glowA(): Resource {
    return $r('app.color.ol_glow_a');
  }

  static glowB(): Resource {
    return $r('app.color.ol_glow_b');
  }

  static glowC(): Resource {
    return $r('app.color.ol_glow_c');
  }

  static elevated(): Resource {
    return $r('app.color.ol_surface_elevated');
  }

  static sunken(): Resource {
    return $r('app.color.ol_surface_sunken');
  }

  static overlay(): Resource {
    return $r('app.color.ol_overlay');
  }

  static border(): Resource {
    return $r('app.color.ol_border');
  }

  static divider(): Resource {
    return $r('app.color.ol_divider');
  }

  static textPrimary(): Resource {
    return $r('app.color.ol_text_primary');
  }

  static textSecondary(): Resource {
    return $r('app.color.ol_text_secondary');
  }

  static textTertiary(): Resource {
    return $r('app.color.ol_text_tertiary');
  }

  static textOnAccent(): Resource {
    return $r('app.color.ol_text_on_accent');
  }

  static iconPrimary(): Resource {
    return $r('app.color.ol_icon_primary');
  }

  static iconSecondary(): Resource {
    return $r('app.color.ol_icon_secondary');
  }

  static accent(): Resource {
    return $r('app.color.ol_accent');
  }

  static accentSoft(): Resource {
    return $r('app.color.ol_accent_soft');
  }

  static accentPressed(): Resource {
    return $r('app.color.ol_accent_pressed');
  }

  static success(): Resource {
    return $r('app.color.ol_success');
  }

  static successSoft(): Resource {
    return $r('app.color.ol_success_soft');
  }

  static warning(): Resource {
    return $r('app.color.ol_warning');
  }

  static warningSoft(): Resource {
    return $r('app.color.ol_warning_soft');
  }

  static danger(): Resource {
    return $r('app.color.ol_danger');
  }

  static dangerSoft(): Resource {
    return $r('app.color.ol_danger_soft');
  }

  static pressed(): Resource {
    return $r('app.color.ol_pressed');
  }

  static hover(): Resource {
    return $r('app.color.ol_hover');
  }

  static scrim(): Resource {
    return $r('app.color.ol_scrim');
  }

  /** Activity heat map ramp, 0 = no dictation that day. */
  static heat(level: number): Resource {
    if (level <= 0) return $r('app.color.ol_heat_0');
    if (level === 1) return $r('app.color.ol_heat_1');
    if (level === 2) return $r('app.color.ol_heat_2');
    if (level === 3) return $r('app.color.ol_heat_3');
    return $r('app.color.ol_heat_4');
  }
}

/** Spacing on an 8vp grid. */
export class S {
  static readonly xxs: number = 2;
  static readonly xs: number = 4;
  static readonly sm: number = 8;
  static readonly md: number = 12;
  static readonly lg: number = 16;
  static readonly xl: number = 24;
  static readonly xxl: number = 32;
  static readonly xxxl: number = 48;

  /** Page gutter: phones keep 16vp, wider layouts breathe. */
  static gutter(bp: string): number {
    if (bp === 'lg') return 32;
    if (bp === 'md') return 24;
    return 16;
  }

  /** Sidebar / rail width by size class. */
  static rail(bp: string): number {
    if (bp === 'lg') return 240;
    if (bp === 'md') return 88;
    return 0;
  }
}

/** Corner radii. */
export class R {
  static readonly xs: number = 6;
  static readonly sm: number = 8;
  static readonly md: number = 12;
  static readonly lg: number = 16;
  static readonly xl: number = 24;
  static readonly pill: number = 999;
}

/** Type scale (fp). */
export class T {
  static readonly display: number = 30;
  static readonly title: number = 22;
  static readonly headline: number = 18;
  static readonly body: number = 16;
  static readonly callout: number = 15;
  static readonly footnote: number = 13;
  static readonly caption: number = 12;
}

/** Shared card shadow; flat surfaces on dark stay flat. */
export function cardShadow(): ShadowOptions {
  return {
    radius: 16,
    color: '#12000000',
    offsetX: 0,
    offsetY: 4
  };
}
