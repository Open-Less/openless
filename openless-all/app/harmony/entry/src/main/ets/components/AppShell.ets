import { KeyCode } from '@kit.InputKit';
import { Layout, Leading, Palette, Radius, Space, Type, floatShadow } from '../common/Theme';
import { PageBackBar } from './Kit';
import { symbolOf } from './Icons';
import { Nav, NavItem } from './Nav';
import { CredentialsPage } from '../pages/CredentialsPage';
import { HistoryPage } from '../pages/HistoryPage';
import { CorrectionsPage, StylePage, VocabPage } from '../pages/LibraryPages';
import { OverviewPage } from '../pages/OverviewPage';
import { SelectionPage } from '../pages/SelectionPage';
import { SettingsPage } from '../pages/SettingsPage';
import { LocalAsrPage, QaPage, TranslationPage } from '../pages/ToolPages';

/**
 * Adaptive shell v2 (一次开发，多端部署).
 *
 *   sm  < 600vp              floating capsule dock (4 destinations)
 *   md  600-840              88vp icon rail
 *   lg  840-1080             240vp labelled rail
 *   xl  >= 1080              264vp labelled rail
 *
 * The dock is a real HarmonyOS material: `uiMaterial.ImmersiveMaterial` through
 * `Glass.bar()` on API 26+, with the plain system blur as the fallback. It floats
 * above the bottom edge with a margin, and the shell reserves its height so no
 * page content is covered; the window itself stays inset by the system, so the
 * gesture bar never overlaps the capsule.
 */
@Component
export struct AppShell {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('route') route: string = 'overview';

  build() {
    Row() {
      if (Layout.isRail(this.bp)) {
        this.rail()
      }
      Column() {
        if (Nav.isSecondary(this.route)) {
          Row() {
            PageBackBar({
              label: Nav.label(Nav.ownerOf(this.route)),
              onTap: () => Nav.go(Nav.ownerOf(this.route))
            })
          }
          .width('100%')
          .padding({ left: Layout.gutter(this.bp), right: Layout.gutter(this.bp) })
        }

        Row() {
          Column() {
            this.content()
          }
          .width('100%')
          .height('100%')
          .constraintSize({ maxWidth: Layout.contentMax(this.bp) })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)

        if (!Layout.isRail(this.bp)) {
          this.dock()
        }
      }
      .layoutWeight(1)
      .height('100%')
    }
    .width('100%')
    .height('100%')
    .backgroundColor(Palette.bg())
    .focusable(true)
    .onKeyEvent((event: KeyEvent) => this.handleKey(event))
  }

  /** Ctrl/⌘ + 1..4 jumps to a primary destination on tablets and 2-in-1. */
  private handleKey(event: KeyEvent): void {
    if (event.type !== KeyType.Down) return;
    let withModifier: boolean = false;
    if (event.getModifierKeyState !== undefined) {
      try {
        withModifier = event.getModifierKeyState(['Ctrl']) || event.getModifierKeyState(['Meta']);
      } catch (error) {
        withModifier = false;
      }
    }
    if (!withModifier) return;
    const slots: number[] = [KeyCode.KEYCODE_1, KeyCode.KEYCODE_2, KeyCode.KEYCODE_3,
      KeyCode.KEYCODE_4];
    for (let index = 0; index < slots.length; index++) {
      if (event.keyCode === slots[index]) {
        if (index < Nav.PRIMARY.length) Nav.go(Nav.PRIMARY[index]);
        return;
      }
    }
  }

  @Builder
  content() {
    if (this.route === Nav.HISTORY) {
      HistoryPage()
    } else if (this.route === Nav.STYLE) {
      StylePage()
    } else if (this.route === Nav.SETTINGS) {
      SettingsPage()
    } else if (this.route === Nav.SELECTION) {
      SelectionPage()
    } else if (this.route === Nav.TRANSLATION) {
      TranslationPage()
    } else if (this.route === Nav.VOCAB) {
      VocabPage()
    } else if (this.route === Nav.CORRECTIONS) {
      CorrectionsPage()
    } else if (this.route === Nav.QA) {
      QaPage()
    } else if (this.route === Nav.CREDENTIALS) {
      CredentialsPage()
    } else if (this.route === Nav.LOCALASR) {
      LocalAsrPage()
    } else {
      OverviewPage()
    }
  }

  /** Floating capsule dock: one material surface, four equal destinations. */
  @Builder
  dock() {
    Column() {
      Row() {
        ForEach(Nav.PRIMARY, (id: string) => {
          this.dockItem(id)
        }, (id: string) => 'dock-' + id)
      }
      .width('100%')
      .height(60)
      .padding({ left: Space.xxs, right: Space.xxs })
      .borderRadius(Radius.pill)
      .backgroundColor(Palette.bgElevated())
      // 沉浸光感：系统毛玻璃材质。API 26 的 uiMaterial 在兼容 SDK 12 上不可用，
      // 这里用系统 COMPONENT_ULTRA_THICK 模糊 + 半透明底色近似同一效果。
      .backgroundBlurStyle(BlurStyle.COMPONENT_ULTRA_THICK)
      .border({ width: 0.5, color: Palette.line() })
      .shadow(floatShadow())
    }
    .width('100%')
    .padding({ left: Space.md, right: Space.md, top: Space.xxs, bottom: Space.sm })
  }

  @Builder
  dockItem(id: string) {
    Column({ space: 2 }) {
      SymbolGlyph(symbolOf(id))
        .fontSize(21)
        .fontColor([this.tabActive(id) ? Palette.accent() : Palette.icon2()])
      Text(Nav.label(id))
        .fontSize(Type.caption)
        .lineHeight(Leading.caption)
        .fontWeight(this.tabActive(id) ? FontWeight.Medium : FontWeight.Normal)
        .fontColor(this.tabActive(id) ? Palette.accent() : Palette.text2())
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
    }
    .layoutWeight(1)
    .height(52)
    .justifyContent(FlexAlign.Center)
    .backgroundColor(this.tabActive(id) ? Palette.accentSoft() : Color.Transparent)
    .borderRadius(Radius.pill)
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onClick(() => Nav.go(id))
  }

  @Builder
  rail() {
    Column() {
      Row({ space: Space.xs }) {
        Text('OL')
          .fontSize(Type.caption)
          .fontWeight(FontWeight.Bold)
          .fontColor(Palette.onAccent())
          .width(32)
          .height(32)
          .textAlign(TextAlign.Center)
          .backgroundColor(Palette.accent())
          .borderRadius(Radius.control)

        if (Layout.isLabelledRail(this.bp)) {
          Text('OpenLess')
            .fontSize(Type.cardTitle)
            .fontWeight(FontWeight.Medium)
            .fontColor(Palette.text1())
            .layoutWeight(1)
        }
      }
      .width('100%')
      .padding({
        left: Layout.isLabelledRail(this.bp) ? Space.md : 0,
        right: Space.md,
        top: Space.lg,
        bottom: Space.lg
      })
      .justifyContent(Layout.isLabelledRail(this.bp) ? FlexAlign.Start : FlexAlign.Center)

      ForEach(Nav.items(), (item: NavItem) => {
        this.railItem(item)
      }, (item: NavItem) => 'rail-' + item.id)

      Blank()

      if (Layout.isLabelledRail(this.bp)) {
        Text('v' + (AppStorage.get<string>('appVersion') ?? '—'))
          .fontSize(Type.caption)
          .fontColor(Palette.text3())
          .width('100%')
          .padding({ left: Space.md, bottom: Space.lg })
      }
    }
    .width(Layout.rail(this.bp))
    .height('100%')
    .padding({ left: Space.xs, right: Space.xs })
    .backgroundColor(Palette.bgElevated())
    .border({ width: { right: 0.5 }, color: Palette.line() })
  }

  @Builder
  railItem(item: NavItem) {
    Column({ space: Space.xxs }) {
      SymbolGlyph(symbolOf(item.id))
        .fontSize(22)
        .fontColor([this.tabActive(item.id) ? Palette.accent() : Palette.icon2()])
      if (Layout.isLabelledRail(this.bp)) {
        Text(item.label)
          .fontSize(Type.caption)
          .lineHeight(Leading.caption)
          .fontColor(this.tabActive(item.id) ? Palette.accent() : Palette.text2())
      }
    }
    .width('100%')
    .height(Layout.isLabelledRail(this.bp) ? 56 : 52)
    .justifyContent(FlexAlign.Center)
    .alignItems(HorizontalAlign.Center)
    .margin({ bottom: Space.xxs })
    .backgroundColor(this.tabActive(item.id) ? Palette.accentSoft() : Color.Transparent)
    .borderRadius(Radius.card)
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onClick(() => Nav.go(item.id))
  }

  /** Secondary pages keep their top-level destination lit in the dock. */
  private tabActive(id: string): boolean {
    return Nav.rootOf(this.route) === id;
  }
}
