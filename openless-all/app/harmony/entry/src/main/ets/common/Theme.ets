/**
 * v2 design tokens for the native HarmonyOS UI.
 *
 * Rules (see docs/harmony-ui-v2.md):
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
  static readonly control: number = 8;
  static readonly card: number = 12;
  static readonly sheet: number = 16;
  static readonly pill: number = 999;
}

/** Type scale (fp). Sizes only; use `Leading` next to them for block text. */
export class Type {
  static readonly pageTitle: number = 30;
  static readonly section: number = 20;
  static readonly cardTitle: number = 17;
  static readonly body: number = 16;
  static readonly label: number = 14;
  static readonly caption: number = 12;
  static readonly metric: number = 40;
}

/** Line heights paired with `Type`. */
export class Leading {
  static readonly pageTitle: number = 38;
  static readonly section: number = 26;
  static readonly cardTitle: number = 24;
  static readonly body: number = 24;
  static readonly label: number = 20;
  static readonly caption: number = 16;
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
export class Motion {
  static readonly quick: number = 120;
  static readonly base: number = 180;
  static readonly slow: number = 240;
}

/** Breakpoint-dependent layout metrics. */
export class Layout {
  static readonly SM: string = 'sm';
  static readonly MD: string = 'md';
  static readonly LG: string = 'lg';
  static readonly XL: string = 'xl';

  /** Page side margin: 16 / 24 / 32. */
  static gutter(bp: string): number {
    if (bp === Layout.XL) return 32;
    if (bp === Layout.LG) return 24;
    return 16;
  }

  /** Max width of a single content column. */
  static contentMax(bp: string): number {
    if (bp === Layout.SM) return 10000;
    if (bp === Layout.MD) return 640;
    if (bp === Layout.LG) return 760;
    return 880;
  }

  /** Max width of a two-column page body. */
  static wideMax(bp: string): number {
    return bp === Layout.XL ? 1200 : 1040;
  }

  /** Navigation rail width; 0 means the bottom tab bar. */
  static rail(bp: string): number {
    if (bp === Layout.XL) return 264;
    if (bp === Layout.LG) return 240;
    if (bp === Layout.MD) return 88;
    return 0;
  }

  /** True when navigation is a side rail rather than a bottom tab bar. */
  static isRail(bp: string): boolean {
    return bp === Layout.MD || bp === Layout.LG || bp === Layout.XL;
  }

  /** True when the rail carries labels instead of icons only. */
  static isLabelledRail(bp: string): boolean {
    return bp === Layout.LG || bp === Layout.XL;
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

/**
 * 手机端优先：当前阶段只做手机竖屏的样子。开启时窗口尺寸类固定为 sm——
 * 不画侧栏、页面单列、主页卡片一列、历史/设置不分栏；等手机形态定稿后再打开
 * 宽度自适应（改为 false 即可恢复 md/lg/xl）。
 */
export const PHONE_FIRST: boolean = true;
