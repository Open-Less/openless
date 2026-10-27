import { Breakpoints } from '../common/Breakpoint';
import { C, R, S, T } from '../common/Tokens';
import { CredentialsPage } from '../pages/CredentialsPage';
import { HistoryPage } from '../pages/HistoryPage';
import { CorrectionsPage, MarketplacePage, StylePage, VocabPage } from '../pages/LibraryPages';
import { OverviewPage } from '../pages/OverviewPage';
import { SettingsPage } from '../pages/SettingsPage';
import { LocalAsrPage, QaPage, QuickNotePage, TranslationPage } from '../pages/ToolPages';
import { symbolOf } from './Icons';
import { Nav, NavItem } from './Nav';

/**
 * Adaptive application shell (一次开发，多端部署).
 *
 *   < 840vp   bottom tab bar (five destinations)     phone portrait/landscape,
 *                                                    tablet portrait
 *   >= 840vp  labelled side bar                      tablet landscape, 2-in-1
 *
 * Only five destinations exist at this level. Secondary screens live inside 设置,
 * which is what keeps a phone-sized tab bar readable and a tablet side bar short.
 * Chrome is drawn on translucent material (`backgroundBlurStyle`) over the ambient
 * glow painted by `Index`, i.e. the HarmonyOS immersive-material look.
 */
@Component
export struct AppShell {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('route') route: string = 'overview';

  build() {
    Row() {
      if (Breakpoints.isWide(this.bp)) {
        this.sideBar()
      }
      Column() {
        Column() {
          this.content()
        }
        .layoutWeight(1)
        .width('100%')

        if (!Breakpoints.isWide(this.bp)) {
          this.tabBar()
        }
      }
      .layoutWeight(1)
      .height('100%')
    }
    .width('100%')
    .height('100%')
  }

  /** Route table: navigation only rebuilds this slot, pages keep their own state. */
  @Builder
  content() {
    if (this.route === Nav.HISTORY) {
      HistoryPage()
    } else if (this.route === Nav.STYLE) {
      StylePage()
    } else if (this.route === Nav.VOCAB) {
      VocabPage()
    } else if (this.route === Nav.SETTINGS) {
      SettingsPage()
    } else if (this.route === Nav.CREDENTIALS) {
      CredentialsPage()
    } else if (this.route === Nav.MARKETPLACE) {
      MarketplacePage()
    } else if (this.route === Nav.CORRECTIONS) {
      CorrectionsPage()
    } else if (this.route === Nav.TRANSLATION) {
      TranslationPage()
    } else if (this.route === Nav.QA) {
      QaPage()
    } else if (this.route === Nav.QUICKNOTE) {
      QuickNotePage()
    } else if (this.route === Nav.LOCALASR) {
      LocalAsrPage()
    } else {
      OverviewPage()
    }
  }

  @Builder
  sideBar() {
    Column() {
      Row({ space: S.md }) {
        Text('OL')
          .fontSize(T.footnote)
          .fontWeight(FontWeight.Bold)
          .fontColor(C.textOnAccent())
          .width(32)
          .height(32)
          .textAlign(TextAlign.Center)
          .backgroundColor(C.accent())
          .borderRadius(R.md)
        Column({ space: 0 }) {
          Text('OpenLess')
            .fontSize(T.body)
            .fontWeight(FontWeight.Medium)
            .fontColor(C.textPrimary())
          Text('鸿蒙原生版')
            .fontSize(T.caption)
            .fontColor(C.textTertiary())
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
      }
      .width('100%')
      .padding({ top: S.xl, bottom: S.xl, left: S.sm, right: S.sm })

      ForEach(Nav.items(), (item: NavItem) => {
        this.railItem(item)
      }, (item: NavItem) => item.id)

      Blank()

      Text('v' + (AppStorage.get<string>('appVersion') ?? '—'))
        .fontSize(T.caption)
        .fontColor(C.textTertiary())
        .padding({ left: S.md, bottom: S.lg })
    }
    .width(S.rail(this.bp))
    .height('100%')
    .padding({ left: S.sm, right: S.sm })
    .backgroundColor(C.glass())
    .backgroundBlurStyle(BlurStyle.COMPONENT_ULTRA_THICK)
    .border({ width: { right: 0.5 }, color: C.border() })
  }

  @Builder
  railItem(item: NavItem) {
    Row({ space: S.md }) {
      SymbolGlyph(symbolOf(item.id))
        .fontSize(20)
        .fontColor([this.tabActive(item.id) ? C.accent() : C.iconSecondary()])
      Text(item.label)
        .fontSize(T.callout)
        .fontColor(this.tabActive(item.id) ? C.accent() : C.textPrimary())
        .fontWeight(this.tabActive(item.id) ? FontWeight.Medium : FontWeight.Regular)
        .layoutWeight(1)
    }
    .width('100%')
    .height(S.rowSingle)
    .padding({ left: S.md, right: S.md })
    .margin({ bottom: S.xxs })
    .backgroundColor(this.tabActive(item.id) ? C.accentSoft() : Color.Transparent)
    .borderRadius(R.md)
    // 2-in-1 / PC: pointer hover and keyboard focus must be visible before the click.
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onClick(() => Nav.go(item.id))
  }

  @Builder
  tabBar() {
    Row() {
      ForEach(Nav.PRIMARY, (id: string) => {
        this.tabItem(id)
      }, (id: string) => 'tab-' + id)
    }
    .width('100%')
    .height(64)
    .padding({ left: S.sm, right: S.sm, bottom: S.xs })
    .backgroundColor(C.glassStrong())
    .backgroundBlurStyle(BlurStyle.COMPONENT_ULTRA_THICK)
    .border({ width: { top: 0.5 }, color: C.border() })
  }

  @Builder
  tabItem(id: string) {
    Column({ space: S.xxs }) {
      SymbolGlyph(symbolOf(id))
        .fontSize(22)
        .fontColor([this.tabActive(id) ? C.accent() : C.iconSecondary()])
      Text(Nav.label(id))
        .fontSize(T.caption)
        .fontColor(this.tabActive(id) ? C.accent() : C.textSecondary())
        .fontWeight(this.tabActive(id) ? FontWeight.Medium : FontWeight.Regular)
    }
    .layoutWeight(1)
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .hoverEffect(HoverEffect.Highlight)
    .focusable(true)
    .onClick(() => Nav.go(id))
  }

  /** A secondary page keeps its owner tab lit (风格市场 → 风格, 纠正规则 → 词典). */
  private tabActive(id: string): boolean {
    return Nav.ownerOf(this.route) === id;
  }
}
