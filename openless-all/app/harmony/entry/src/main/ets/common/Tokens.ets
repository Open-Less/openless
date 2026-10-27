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
  // HarmonyOS 8vp grid: 4 / 8 / 12 / 16 / 24 / 32 / 48 / 64.
  static readonly xxs: number = 4;
  static readonly xs: number = 8;
  static readonly sm: number = 12;
  static readonly md: number = 16;
  static readonly lg: number = 24;
  static readonly xl: number = 32;
  static readonly xxl: number = 48;
  static readonly xxxl: number = 64;
  /** Minimum touch target (spec: 40vp, recommended 48vp). */
  static readonly touch: number = 44;
  /** Single-line list row / two-line row. */
  static readonly rowSingle: number = 48;
  static readonly rowDouble: number = 64;

  /** Page margin: SM 16 / MD-LG 24 / XL 32 (spec table). */
  static gutter(bp: string): number {
    if (bp === 'xl') return 32;
    if (bp === 'lg' || bp === 'md') return 24;
    return 16;
  }

  /** Column gutter between cards in a multi-column layout. */
  static columnGap(bp: string): number {
    return bp === 'sm' ? 16 : 24;
  }

  /** Side bar width; 0 means no side bar (bottom tab bar instead). */
  static rail(bp: string): number {
    if (bp === 'xl') return 288;
    if (bp === 'lg') return 264;
    return 0;
  }
}

/** Corner radii. */
export class R {
  static readonly xs: number = 4;
  /** Spec S: small buttons, inputs, tags. */
  static readonly sm: number = 8;
  /** Spec M: cards, list items, dialogs. */
  static readonly md: number = 16;
  /** Spec L: large cards, bottom sheets. */
  static readonly lg: number = 24;
  /** Spec XL: page-level containers. */
  static readonly xl: number = 32;
  static readonly pill: number = 999;
}

/** Type scale (fp). */
/**
 * HarmonyOS type scale (fp). Line heights follow the spec table; use `Lh` next to
 * these when a block needs explicit leading.
 */
export class T {
  static readonly display: number = 32;   // Headline L for page titles
  static readonly title: number = 24;     // Headline S: card/section titles
  static readonly headline: number = 20;  // Title L: list/section headings
  static readonly body: number = 16;      // Body M: default text
  static readonly callout: number = 17;   // Title M / Body L
  static readonly footnote: number = 14;  // Body S: helper text
  static readonly caption: number = 12;   // Caption: timestamps
  static readonly hero: number = 48;      // Display: big numbers only
}

export class Lh {
  static readonly hero: number = 60;
  static readonly display: number = 40;
  static readonly title: number = 32;
  static readonly headline: number = 28;
  static readonly body: number = 24;
  static readonly footnote: number = 20;
  static readonly caption: number = 16;
}

/** Spec Level 2 shadow (card elevation): blur 8vp, 15% black, no Y offset. */
export function cardShadow(): ShadowOptions {
  return {
    radius: 8,
    color: '#26000000',
    offsetX: 0,
    offsetY: 2
  };
}

/** Spec Level 3 shadow for sheets and modals: blur 16vp, 20% black. */
export function sheetShadow(): ShadowOptions {
  return {
    radius: 16,
    color: '#33000000',
    offsetX: 0,
    offsetY: 4
  };
}
