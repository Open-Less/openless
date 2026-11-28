import { deviceInfo } from '@kit.BasicServicesKit';
import { uiMaterial } from '@kit.ArkUI';
import { Leading, Motion, Palette, Radius, Size, Space, Type, floatShadow } from '../common/Theme';
import { Store } from '../common/Store';
import { dockImageOf, hasDockImage, symbolOf } from './Icons';
import { Nav } from './Nav';

// ProductDock.swift:186-193 keeps the isolated platter circular at 62pt.
const DOCK_HEIGHT: number = 62;
const DOCK_GAP: number = 10;

/** Native Button gives touch, keyboard activation and focus one shared action.
 * Huawei multi-input guidance:
 * https://developer.huawei.com/consumer/cn/doc/best-practices-V14/bpta-multi-interaction-V14
 */
/**
 * iOS `ProductDock` (ProductDock.swift), frame-verified against the demo video:
 *
 *   collapsed  one pill (概览/历史/词典/设置) + a separate Styles pill
 *   expanded   a circular Dictionary platter + a three-item Styles platter:
 *              Dict… | My Style | Marketplace | Translation targets
 *
 * Selected item = accentSoft capsule with accent glyph+label; unselected glyphs
 * use the primary text colour (black in light, white in dark), not grey.
 * Expansion animates 0.28s with a 0.12s easeOut handoff (ProductDock.swift:149,159).
 * https://developer.huawei.com/consumer/cn/doc/harmonyos-references/ts-animatorproperty
 */
@Component
export struct ProductDock {
  @Prop route: string = Nav.OVERVIEW;
  @StorageLink('styleSection') section: number = 0;
  @StorageLink('reduceMotion') reduceMotion: boolean = false;
  private mat: SystemUiMaterial | undefined = undefined;

  aboutToAppear(): void {
    this.mat = this.dockMaterial();
  }

  private hasMaterial(): boolean {
    return this.mat !== undefined;
  }

  private pillFill(): ResourceColor {
    return this.hasMaterial() ? Color.Transparent : Palette.glassLo();
  }

  /** Deep glass gradient when we paint it ourselves; transparent under material. */
  private pillGradient(): LinearGradientOptions {
    if (this.hasMaterial()) {
      return { angle: 180, colors: [[Color.Transparent, 0], [Color.Transparent, 1]] };
    }
    return { angle: 180, colors: [[Palette.glassHi(), 0], [Palette.glassLo(), 1]] };
  }

  private pillBlur(): BlurStyle {
    return this.hasMaterial() ? BlurStyle.NONE : BlurStyle.COMPONENT_ULTRA_THICK;
  }

  /** The styles pill stays expanded for its four sections, 翻译目标 included. */
  private expanded(): boolean {
    return Nav.rootOf(this.route) === Nav.STYLE || this.route === Nav.TRANSLATION;
  }

  /**
   * System immersive material for the floating dock (the「沉浸光感」look used by
   * Melotopia and HarmonyOS first-party apps). It is an API 26 surface, so older
   * devices keep the blur + gradient fallback painted below.
   * https://developer.huawei.com/consumer/cn/doc/harmonyos-references/js-apis-uimaterial
   */
  private dockMaterial(): SystemUiMaterial | undefined {
    try {
      if (deviceInfo.sdkApiVersion >= 26 && uiMaterial.isImmersiveMaterialSupported()) {
        const material = new uiMaterial.ImmersiveMaterial({
          style: uiMaterial.ImmersiveStyle.THIN,
          materialColor: Palette.dockTint(),
          applyShadow: true,
          interactive: false
        });
        console.info('[OpenLess Dock] immersive material active, api=' + deviceInfo.sdkApiVersion.toString());
        return material;
      }
      console.info('[OpenLess Dock] immersive material needs API 26, api=' + deviceInfo.sdkApiVersion.toString());
    } catch (error) {
      console.warn('[OpenLess Dock] immersive material unavailable');
    }
    return undefined;
  }

  private openStyle(section: number): void {
    Store.put<number>('styleSection', section);
    Nav.go(Nav.STYLE);
  }

  /** iOS swaps the whole dock with a spring, not a crossfade. */
  private animateDock(action: () => void): void {
    if (this.reduceMotion) {
      animateTo({ duration: Motion.reduced, curve: Curve.EaseOut }, () => {
        action();
      });
      return;
    }
    animateTo({ duration: Motion.dock, curve: Curve.Friction }, () => {
      action();
    });
  }

  /** ForEach retains each item; read the live route inside its reactive
   * attributes instead of capturing a computed Boolean when the item is created.
   */
  private primaryActive(id: string): boolean {
    return !this.expanded() && Nav.rootOf(this.route) === id;
  }

  build() {
    // iOS renders two bars: the primary pill and the styles pill. Collapsed =
    // 4 + 1; expanded = 词典 + the three style sections (ProductDock.swift:13-24).
    Row({ space: DOCK_GAP }) {
      Row() {
        if (this.expanded()) {
          this.item(Nav.VOCAB, '词典', 'dict')
        } else {
          ForEach(Nav.DOCK_MAIN, (id: string) => {
            this.item(id, Nav.label(id), id)
          }, (id: string) => 'dock-' + id)
        }
      }
      .id('dock-primary-platter')
      .layoutWeight(this.expanded() ? 0 : 1)
      .width(this.expanded() ? DOCK_HEIGHT : undefined)
      .constraintSize({ minHeight: DOCK_HEIGHT })
      .padding(Space.xxs)
      .linearGradient(this.pillGradient())
      .backgroundColor(this.pillFill())
      .backgroundBlurStyle(this.pillBlur())
      .borderRadius(Radius.pill)
      .border({ width: 0.5, color: Palette.glassEdge() })
      .shadow(floatShadow())
      .systemMaterial(this.mat)

      Row() {
        if (this.expanded()) {
          this.styleItem('我的风格', 'style', 0)
          this.styleItem('风格市场', 'marketplace', 1)
          this.styleItem('翻译目标', 'translation', 2)
        } else {
          this.item(Nav.STYLE, '风格', 'style')
        }
      }
      .id('dock-style-platter')
      .layoutWeight(this.expanded() ? 1 : 0)
      .width(this.expanded() ? undefined : DOCK_HEIGHT)
      .constraintSize({ minHeight: DOCK_HEIGHT })
      .padding(Space.xxs)
      .linearGradient(this.pillGradient())
      .backgroundColor(this.pillFill())
      .backgroundBlurStyle(this.pillBlur())
      .borderRadius(Radius.pill)
      .border({ width: 0.5, color: Palette.glassEdge() })
      .shadow(floatShadow())
      .systemMaterial(this.mat)
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.xs, bottom: Space.xs })
  }

  @Builder
  item(id: string, label: string, key: string) {
    Button({ type: ButtonType.Normal }) {
      Column({ space: 2 }) {
        if (hasDockImage(id)) {
          Image(dockImageOf(id))
            .width(Size.icon).height(Size.icon)
            .objectFit(ImageFit.Contain)
            .fillColor(this.primaryActive(id) ? Palette.accent() : Palette.text1())
            .accessibilityLevel('no')
        } else {
          SymbolGlyph(symbolOf(id)).fontSize(Size.icon)
            .fontColor([this.primaryActive(id) ? Palette.accent() : Palette.text1()])
        }
        Text(label).fontSize(Type.caption2).lineHeight(Leading.caption2)
          .fontColor(this.primaryActive(id) ? Palette.accent() : Palette.text2())
          .maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
    }
    .id('dock-' + key)
    .layoutWeight(1)
    .constraintSize({ minHeight: 54 })
    .borderRadius(Radius.pill)
    .backgroundColor(this.primaryActive(id) ? Palette.accentSoft() : Color.Transparent)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(label)
    .accessibilitySelected(this.primaryActive(id))
    .onClick(() => {
      if (id === Nav.STYLE) {
        this.animateDock(() => {
          Nav.openPrimary(Nav.STYLE);
        });
      } else {
        Nav.openPrimary(id);
      }
    })
  }

  @Builder
  styleItem(label: string, symbol: string, section: number) {
    Button({ type: ButtonType.Normal }) {
      Column({ space: 2 }) {
        if (hasDockImage(symbol)) {
          Image(dockImageOf(symbol))
            .width(Size.icon).height(Size.icon)
            .objectFit(ImageFit.Contain)
            .fillColor(this.styleActive(section) ? Palette.accent() : Palette.text1())
            .accessibilityLevel('no')
        } else {
          SymbolGlyph(symbolOf(symbol)).fontSize(Size.icon)
            .fontColor([this.styleActive(section) ? Palette.accent() : Palette.text1()])
        }
        Text(label).fontSize(Type.caption2).lineHeight(Leading.caption2)
          .fontColor(this.styleActive(section) ? Palette.accent() : Palette.text2())
          .maxLines(1).textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
    }
    .id('dock-style-' + section.toString())
    .layoutWeight(1)
    .constraintSize({ minHeight: 54 })
    .borderRadius(Radius.pill)
    .backgroundColor(this.styleActive(section) ? Palette.accentSoft() : Color.Transparent)
    .hoverEffect(HoverEffect.Highlight)
    .accessibilityText(label)
    .accessibilitySelected(this.styleActive(section))
    .onClick(() => {
      if (section === 2) Nav.go(Nav.TRANSLATION);
      else this.openStyle(section);
    })
  }

  private styleActive(section: number): boolean {
    if (section === 2) return this.route === Nav.TRANSLATION;
    return this.route === Nav.STYLE && this.section === section;
  }
}
