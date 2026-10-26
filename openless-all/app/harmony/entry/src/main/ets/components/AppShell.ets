import { Breakpoints } from '../common/Breakpoint';
import { C, R, S, T } from '../common/Tokens';
import { CredentialsPage } from '../pages/CredentialsPage';
import { HistoryPage } from '../pages/HistoryPage';
import { CorrectionsPage, MarketplacePage, StylePage, VocabPage } from '../pages/LibraryPages';
import { OverviewPage } from '../pages/OverviewPage';
import { SettingsPage } from '../pages/SettingsPage';
import { LocalAsrPage, QaPage, QuickNotePage, TranslationPage } from '../pages/ToolPages';
import { symbolOf } from './Icons';
import { MorePage } from './MorePage';
import { Nav, NavItem } from './Nav';

/**
 * Adaptive application shell (一次开发，多端部署).
 *
 *   sm  bottom tab bar            phone portrait
 *   md  icon rail + content       phone landscape, small tablet, narrow window
 *   lg  labelled sidebar + content tablet, 2-in-1, wide window
 *
 * The shell only owns navigation chrome; each page owns its own header and body,
 * so a page looks the same in a single column and in a two-column wide layout.
 */
@Component
export struct AppShell {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('route') route: string = 'overview';

  build() {
    Row() {
      if (Breakpoints.isCompact(this.bp)) {
        Column() {
          Column() {
            this.content()
          }
          .layoutWeight(1)
          .width('100%')

          this.tabBar()
        }
        .layoutWeight(1)
        .height('100%')
      } else {
        this.sideBar()
        Column() {
          this.content()
        }
        .layoutWeight(1)
        .height('100%')
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(C.bg())
  }

  /**
   * Route table.
   *
   * The switch lives in the shell rather than in `Index`: navigation then only
   * rebuilds the shell's content slot, and the page tree keeps its own state
   * (`@StorageLink`) across switches instead of being torn down with the parent.
   */
  @Builder
  content() {
    if (this.route === Nav.HISTORY) {
      HistoryPage()
    } else if (this.route === Nav.VOCAB) {
      VocabPage()
    } else if (this.route === Nav.CORRECTIONS) {
      CorrectionsPage()
    } else if (this.route === Nav.STYLE) {
      StylePage()
    } else if (this.route === Nav.MARKETPLACE) {
      MarketplacePage()
    } else if (this.route === Nav.TRANSLATION) {
      TranslationPage()
    } else if (this.route === Nav.QA) {
      QaPage()
    } else if (this.route === Nav.QUICKNOTE) {
      QuickNotePage()
    } else if (this.route === Nav.LOCALASR) {
      LocalAsrPage()
    } else if (this.route === Nav.CREDENTIALS) {
      CredentialsPage()
    } else if (this.route === Nav.SETTINGS) {
      SettingsPage()
    } else if (this.route === Nav.MORE) {
      MorePage()
    } else {
      OverviewPage()
    }
  }

  @Builder
  sideBar() {
    Column() {
      Column({ space: S.xxs }) {
        Row({ space: S.sm }) {
          Text('OL')
            .fontSize(T.footnote)
            .fontWeight(FontWeight.Bold)
            .fontColor(C.textOnAccent())
            .width(28)
            .height(28)
            .textAlign(TextAlign.Center)
            .backgroundColor(C.accent())
            .borderRadius(R.sm)
          if (Breakpoints.isWide(this.bp)) {
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
          }
        }
        .width('100%')
        .padding({ top: S.lg, bottom: S.lg, left: Breakpoints.isWide(this.bp) ? S.sm : 0 })
      }
      .width('100%')
      .justifyContent(FlexAlign.Center)

      Scroll() {
        Column({ space: S.xxs }) {
          ForEach(Nav.items(), (item: NavItem) => {
            this.railItem(item)
          }, (item: NavItem) => item.id)
        }
        .width('100%')
        .padding({ bottom: S.lg })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width(S.rail(this.bp))
    .height('100%')
    .padding({ left: S.sm, right: S.sm })
    .backgroundColor(C.surface())
    .border({ width: { right: 0.5 }, color: C.border() })
  }

  @Builder
  railItem(item: NavItem) {
    Row({ space: S.md }) {
      SymbolGlyph(symbolOf(item.id))
        .fontSize(20)
        .fontColor([this.route === item.id ? C.accent() : C.iconSecondary()])
      if (Breakpoints.isWide(this.bp)) {
        Text(item.label)
          .fontSize(T.callout)
          .fontColor(this.route === item.id ? C.accent() : C.textPrimary())
          .fontWeight(this.route === item.id ? FontWeight.Medium : FontWeight.Regular)
      }
    }
    .width('100%')
    .height(44)
    .padding({ left: Breakpoints.isWide(this.bp) ? S.md : 0, right: Breakpoints.isWide(this.bp) ? S.md : 0 })
    .justifyContent(Breakpoints.isWide(this.bp) ? FlexAlign.Start : FlexAlign.Center)
    .backgroundColor(this.route === item.id ? C.accentSoft() : Color.Transparent)
    .borderRadius(R.md)
    .onClick(() => Nav.go(item.id))
  }

  @Builder
  tabBar() {
    Row() {
      ForEach(Nav.PRIMARY, (id: string) => {
        this.tabItem(id)
      }, (id: string) => 'tab-' + id)
      this.tabItem(Nav.MORE)
    }
    .width('100%')
    .height(64)
    .padding({ left: S.sm, right: S.sm, bottom: S.xs })
    .backgroundColor(C.surface())
    .border({ width: { top: 0.5 }, color: C.border() })
  }

  @Builder
  tabItem(id: string) {
    Column({ space: S.xxs }) {
      SymbolGlyph(symbolOf(id))
        .fontSize(22)
        .fontColor([this.tabActive(id) ? C.accent() : C.iconSecondary()])
      Text(id === Nav.MORE ? '更多' : Nav.label(id))
        .fontSize(T.caption)
        .fontColor(this.tabActive(id) ? C.accent() : C.textSecondary())
    }
    .layoutWeight(1)
    .height('100%')
    .justifyContent(FlexAlign.Center)
    .onClick(() => Nav.go(id === Nav.MORE ? Nav.MORE : id))
  }

  private tabActive(id: string): boolean {
    if (id === Nav.MORE) {
      return !Nav.isPrimary(this.route) || this.route === Nav.MORE;
    }
    return this.route === id;
  }
}
