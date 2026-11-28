import { KeyCode } from '@kit.InputKit';
import { Layout, Palette, Space } from '../common/Theme';
import { PageBackBar } from './Kit';
import { Nav } from './Nav';
import { ProductDock } from './ProductNavigation';
import { CredentialsPage } from '../pages/CredentialsPage';
import { HistoryPage } from '../pages/HistoryPage';
import { CorrectionsPage, StylePage, VocabPage } from '../pages/LibraryPages';
import { OverviewPage } from '../pages/OverviewPage';
import { SelectionPage } from '../pages/SelectionPage';
import { SettingsPage } from '../pages/SettingsPage';
import { QaPage, TranslationPage } from '../pages/ToolPages';

import { LocalAsrPage } from '../pages/ModelsPage';

/**
 * Routing and window chrome only. ProductNavigation owns the persistent dock;
 * pages own data; AdaptiveLayout owns reusable content reflow. Breakpoints refer
 * to windows, not device names (Huawei multi-device breakpoint practice).
 */
@Component
export struct AppShell {
  @StorageLink('bp') bp: string = 'sm';
  @StorageLink('route') route: string = 'overview';
  /** Portrait (phone-like) presentation: bottom dock, single column. */
  @StorageLink('phoneLayout') phoneLayout: boolean = true;
  @StorageLink('overlayOpen') overlayOpen: boolean = false;
  @StorageLink('keyboardVisible') keyboardVisible: boolean = false;

  private bodyMax(): number {
    // The credential page owns a full-window scrim; only its form is bounded.
    if (this.route === Nav.CREDENTIALS) return 10000;
    return Layout.bodyMax(this.bp, this.route);
  }

  build() {
    // The system material is supported in an actual bottom TabBar, not a plain
    // Stack content layer. One persistent host owns geometry; Nav owns the
    // five destinations and their detail state without duplicating page trees.
    // https://developer.huawei.com/consumer/cn/doc/doccenter-capabilities/arkts-immersive-light-sense-faq
    Tabs({ barPosition: BarPosition.End }) {
      TabContent() {
      Column() {
        if (Nav.isSecondary(this.route)) {
          Row() {
            PageBackBar({
              label: Nav.label(Nav.backTarget(this.route)),
              onTap: () => Nav.back()
            })
          }
          .width('100%')
          .constraintSize({ maxWidth: this.bodyMax() })
          .enabled(!this.overlayOpen)
          .padding({ left: Layout.gutter(this.bp), right: Layout.gutter(this.bp) })
        }
        Row() {
          Column() {
            this.content()
          }
          .width('100%')
          .height('100%')
          .constraintSize({ maxWidth: this.bodyMax() })
        }
        .width('100%')
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
      }
      .width('100%')
      .height('100%')
      // Keep the full viewport behind the dock. Individual scroll tails reserve
      // its height; shrinking this Column creates an opaque empty bottom strip.
      .id('shell-content-viewport')

      }
      .tabBar(this.floatingDock)
    }
    .barOverlap(true)
    .barHeight(this.overlayOpen || this.keyboardVisible ? 0 : 78)
    .barWidth('100%')
    .barMode(BarMode.Fixed)
    .barBackgroundColor(Color.Transparent)
    .barBackgroundBlurStyle(BlurStyle.NONE)
    .scrollable(false)
    .animationDuration(0)
    .width('100%')
    .height('100%')
    .backgroundColor(Palette.bg())
    .focusable(true)
    .onKeyEvent((event: KeyEvent) => this.handleKey(event))
  }

  @Builder
  floatingDock() {
    Row() {
      ProductDock({ route: this.route })
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
    .id('shell-floating-dock')
    .visibility(this.overlayOpen || this.keyboardVisible ? Visibility.Hidden : Visibility.Visible)
    .enabled(!this.overlayOpen && !this.keyboardVisible)
  }

  /** Native focus bubbling: consume only handled Ctrl/Meta+1..5 shortcuts. */
  private handleKey(event: KeyEvent): void {
    if (event.type !== KeyType.Down) return;
    // getModifierKeyState accepts Ctrl/Alt/Shift only; Meta is a separate field.
    // https://developer.huawei.com/consumer/cn/doc/harmonyos-references/ts-universal-events-key
    let withModifier: boolean = event.metaKey === 1;
    if (!withModifier && event.getModifierKeyState !== undefined) {
      try {
        withModifier = event.getModifierKeyState(['Ctrl']);
      } catch (error) {
        return;
      }
    }
    if (!withModifier) return;
    const slots: number[] = [KeyCode.KEYCODE_1, KeyCode.KEYCODE_2, KeyCode.KEYCODE_3,
      KeyCode.KEYCODE_4, KeyCode.KEYCODE_5];
    for (let index = 0; index < slots.length; index++) {
      if (event.keyCode === slots[index]) {
        Nav.openPrimary(Nav.PRIMARY[index]);
        event.stopPropagation();
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
}
