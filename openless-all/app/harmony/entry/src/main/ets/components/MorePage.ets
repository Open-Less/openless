import { C, R, S, T } from '../common/Tokens';
import { Nav, NavItem } from './Nav';
import { symbolOf } from './Icons';
import { PageTitle } from './Common';

/** Compact-layout overflow: every destination that is not in the tab bar. */
@Component
export struct MorePage {
  @StorageLink('bp') bp: string = 'md';
  @StorageLink('route') route: string = 'overview';

  private overflow(): NavItem[] {
    const all: NavItem[] = Nav.items();
    const rest: NavItem[] = [];
    for (let index = 0; index < all.length; index++) {
      if (!Nav.isPrimary(all[index].id)) rest.push(all[index]);
    }
    return rest;
  }

  build() {
    Column() {
      PageTitle({ title: '更多', subtitle: '工具与风格入口' })

      Scroll() {
        Column({ space: S.sm }) {
          ForEach(this.overflow(), (item: NavItem) => {
            Row({ space: S.md }) {
              SymbolGlyph(symbolOf(item.id))
                .fontSize(20)
                .fontColor([C.accent()])
                .width(28)
              Text(item.label)
                .fontSize(T.body)
                .fontColor(C.textPrimary())
                .layoutWeight(1)
              SymbolGlyph(symbolOf('chevron'))
                .fontSize(14)
                .fontColor([C.textTertiary()])
            }
            .width('100%')
            .height(56)
            .padding({ left: S.lg, right: S.lg })
            .backgroundColor(C.surface())
            .borderRadius(R.md)
            .onClick(() => Nav.go(item.id))
          }, (item: NavItem) => item.id)
        }
        .width('100%')
        .padding({ bottom: S.xxl })
      }
      .scrollBar(BarState.Off)
      .layoutWeight(1)
      .align(Alignment.Top)
    }
    .width('100%')
    .height('100%')
    .padding({ left: S.gutter(this.bp), right: S.gutter(this.bp) })
  }
}
