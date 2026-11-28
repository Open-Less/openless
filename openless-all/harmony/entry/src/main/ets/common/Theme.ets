/**
 * v2 design tokens for the native HarmonyOS UI.
 *
 * Rules:
 *   - Colors are semantic roles resolved through `app.color.c_*` resources, so
 *     light and dark stay in step and pages never hard-code a hex value.
 *   - Spacing is an 8vp grid with a 4vp step for tight pairs.
 *   - Type is a closed scale: pages may only use these sizes.
 *   - Elevation is nearly flat: cards use a hairline, only floating layers get a
 *     shadow.
 */

/** Semantic color roles. Every getter resolves through the resource manager. */
export class Palette {
  /** Canvas behind the content (system gray). */
  static bg(): Resource {
    return $r('app.color.c_bg');
  }

  /** Elevated canvas: sheets, dialogs, the window background on wide screens. */
  static bgElevated(): Resource {
    return $r('app.color.c_bg_elev');
  }

  /** Card / list surface. */
  static card(): Resource {
    return $r('app.color.c_card');
  }

  /** Recessed surface inside a card: sub-panels, inputs, chips. */
  static cardAlt(): Resource {
    return $r('app.color.c_card_alt');
  }

  static cardPressed(): Resource {
    return $r('app.color.c_card_pressed');
  }

  /** Hairline border and separators. */
  static line(): Resource {
    return $r('app.color.c_line');
  }

  /** A border that must stay visible on dark (selected cards, focused inputs). */
  static lineStrong(): Resource {
    return $r('app.color.c_line_strong');
  }

  /** Primary text. */
  static text1(): Resource {
    return $r('app.color.c_text_1');
  }

  /** Secondary text: values, descriptions. */
  static text2(): Resource {
    return $r('app.color.c_text_2');
  }

  /** Tertiary text: captions, hints, placeholders. */
  static text3(): Resource {
    return $r('app.color.c_text_3');
  }

  static onAccent(): Resource {
    return $r('app.color.c_text_on_accent');
  }

  static icon1(): Resource {
    return $r('app.color.c_icon_1');
  }

  static icon2(): Resource {
    return $r('app.color.c_icon_2');
  }

  static accent(): Resource {
    return $r('app.color.c_accent');
  }

  static accentSoft(): Resource {
    return $r('app.color.c_accent_soft');
  }

  static accentPressed(): Resource {
    return $r('app.color.c_accent_pressed');
  }

  static success(): Resource {
    return $r('app.color.c_success');
  }

  static successSoft(): Resource {
    return $r('app.color.c_success_soft');
  }

  static warning(): Resource {
    return $r('app.color.c_warning');
  }

  static warningSoft(): Resource {
    return $r('app.color.c_warning_soft');
  }

  static danger(): Resource {
    return $r('app.color.c_danger');
  }

  static dangerSoft(): Resource {
    return $r('app.color.c_danger_soft');
  }

  /** Segmented-control track and its selected thumb. */
  static control(): Resource {
    return $r('app.color.c_control');
  }

  static controlSelected(): Resource {
    return $r('app.color.c_control_selected');
  }

  static scrim(): Resource {
    return $r('app.color.c_scrim');
  }

  /** Immersive capsule glass: top/bottom gradient stops plus the highlight edge. */
  static glassHi(): Resource {
    return $r('app.color.c_glass_hi');
  }

  static glassLo(): Resource {
    return $r('app.color.c_glass_lo');
  }

  /** Soft tint layered on the dock's system material. */
  static dockTint(): Resource {
    return $r('app.color.c_dock_tint');
  }

  static glassEdge(): Resource {
    return $r('app.color.c_glass_edge');
  }

  /** Activity heat map ramp, 0 = no dictation that day. */
  static heat(level: number): Resource {
    if (level <= 0) return $r('app.color.c_heat_0');
    if (level === 1) return $r('app.color.c_heat_1');
    if (level === 2) return $r('app.color.c_heat_2');
    if (level === 3) return $r('app.color.c_heat_3');
    return $r('app.color.c_heat_4');
  }
}

/** Spacing scale (vp). */
export class Space {
  static readonly xxs: number = 4;
  static readonly xs: number = 8;
  static readonly sm: number = 12;
  static readonly md: number = 16;
  static readonly lg: number = 24;
  static readonly xl: number = 32;
  static readonly xxl: number = 40;
}

/** Corner radii (vp). */
export class Radius {
  /** One product radius (React `--ol-radius: 16`) plus a tighter control step. */
  static readonly control: number = 12;
  static readonly icon: number = 10;
  static readonly card: number = 16;
  static readonly sheet: number = 32;
  static readonly pill: number = 999;
}

/**
 * Type scale copied 1:1 from the iOS AppDesign/SwiftUI text styles so the two
 * apps read the same: body 17, subheadline 15, footnote 13, caption 12,
 * caption2 11, title3 20, title2 22, largeTitle 34.
 */
export class Type {
  /** Navigation bar title (iOS inline title, 17 semibold). */
  static readonly navTitle: number = 17;
  static readonly pageTitle: number = 34;
  static readonly section: number = 20;
  static readonly cardTitle: number = 17;
  static readonly body: number = 17;
  /** subheadline: list values, hints, buttons. */
  static readonly label: number = 15;
  /** footnote: captions, section headers, secondary lines. */
  static readonly caption: number = 13;
  /** caption2: tile detail lines. */
  static readonly caption2: number = 11;
  static readonly metric: number = 40;
}

/** Line heights paired with `Type`. */
export class Leading {
  static readonly navTitle: number = 22;
  static readonly pageTitle: number = 41;
  static readonly section: number = 25;
  static readonly cardTitle: number = 22;
  static readonly body: number = 22;
  static readonly label: number = 20;
  static readonly caption: number = 18;
  static readonly caption2: number = 14;
  static readonly metric: number = 46;
}

/** Common control sizes (vp). */
export class Size {
  static readonly touch: number = 44;
  static readonly row: number = 56;
  static readonly rowDouble: number = 72;
  static readonly icon: number = 20;
  static readonly iconSmall: number = 16;
  static readonly iconLarge: number = 28;
  static readonly tile: number = 40;
}

/** Motion durations (ms). Curves stay on `Curve.EaseOut` / `Curve.Friction`. */
/**
 * Motion tokens copied from iOS `ProductMotion` / `PressFeedbackStyle`:
 * quick 160ms easeOut, standard spring(0.34, 0.88), reduced 120ms.
 * ArkUI has no spring(response:damping:) constructor, so `standard` maps to a
 * friction curve with the same visible settling time; reduce-motion callers use
 * `reduced`.
 * https://developer.huawei.com/consumer/cn/doc/harmonyos-references/ts-animatorproperty
 */
export class Motion {
  /** iOS quick = easeOut 0.16. */
  static readonly quick: number = 160;
  /** Button press feedback (iOS 0.14s, scale 0.98 / opacity 0.8). */
  static readonly press: number = 140;
  /** iOS standard spring settles in roughly 340ms. */
  static readonly base: number = 340;
  static readonly slow: number = 240;
  /** iOS `reduced` = easeOut 0.12. */
  static readonly reduced: number = 120;
  /** iOS Dock expand: UIViewPropertyAnimator(duration: 0.28, dampingRatio: 1). */
  static readonly dock: number = 280;
  /** iOS Dock handoff: 0.12 easeOut. */
  static readonly dockHandoff: number = 120;
}

/** Breakpoint-dependent layout metrics. */
export class Layout {
  static readonly SM: string = 'sm';
  static readonly MD: string = 'md';
  static readonly LG: string = 'lg';
  static readonly XL: string = 'xl';

  /**
   * Width of the centred column used while a device presents the phone-style UI
   * (portrait tablet / folded phone). A phone keeps the full window; a tablet gets
   * a wider but still bounded column so cards spread out instead of hugging a
   * 393pt phone strip (Huawei Weather uses the same idea).
   */
  static columnMax(bp: string): number {
    if (bp === Layout.XL) return 880;
    if (bp === Layout.LG) return 720;
    if (bp === Layout.MD) return 640;
    return 10000;
  }

  /** iOS uses a 20pt page margin everywhere; wide windows get a little more. */
  static gutter(bp: string): number {
    if (bp === Layout.XL) return 32;
    if (bp === Layout.LG) return 24;
    return 20;
  }

  /** Max width of a single content column. */
  static contentMax(bp: string): number {
    if (bp === Layout.SM) return 10000;
    if (bp === Layout.MD) return 640;
    if (bp === Layout.LG) return 760;
    return 880;
  }

  /** Floating dock + outer spacing; apply only to scrolling content tails. */
  static dockClearance(): number {
    return 62 + Space.xs * 2 + Space.sm;
  }

  /** Reading columns and multi-pane workspaces have separate width budgets. */
  static wideMax(bp: string): number {
    return bp === Layout.XL ? 1320 : 1120;
  }

  static isWide(bp: string): boolean {
    return bp === Layout.LG || bp === Layout.XL;
  }

  static bodyMax(bp: string, route: string): number {
    // Product destinations fill a tablet's window. Only long-form setup and
    // credential forms keep a reading-width limit, not the page background.
    if (route === 'credentials' || route === 'selection' || route === 'qa') {
      return Layout.contentMax(bp);
    }
    return 10000;
  }

  static columnGap(bp: string): number {
    return bp === Layout.SM ? 12 : 16;
  }
}

/** Level 2 elevation: floating layers only (sheets, dialogs, floating keys). */
export function floatShadow(): ShadowOptions {
  return {
    radius: 24,
    color: '#1F000000',
    offsetX: 0,
    offsetY: 8
  };
}
