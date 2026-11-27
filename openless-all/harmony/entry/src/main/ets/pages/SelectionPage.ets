import { Layout, Leading, Palette, Space, Type } from '../common/Theme';
import { Banner, Group, PageScaffold, RowItem, Surface } from '../components/Kit';
import { Nav } from '../components/Nav';
import { Api } from '../data/Api';
import { PlatformCapabilities } from '../data/Types';

/**
 * 选取助手（划词追问）.
 *
 * The desktop/Android clients register a selection surface; the HarmonyOS app
 * ships as an IME-only build and does not install an accessibility extension, so
 * there is no cross-app selection asset to register. This page states that
 * boundary instead of offering a switch that cannot work, and points at the
 * entries that do work (翻译 / 问答 / 听写).
 */
@Component
export struct SelectionPage {
  @StorageLink('bp') bp: string = 'md';
  @State private capabilities: PlatformCapabilities = { platform: 'harmony' };
  @State private error: string = '';

  aboutToAppear(): void {
    this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.capabilities = await Api.platformCapabilities();
      this.error = '';
    } catch (error) {
      this.error = (error as Error).message;
    }
  }

  build() {
    PageScaffold({
      title: '选取助手',
      subtitle: '选中文字后追问、翻译或改写'
    }) {
      Banner({
        title: '鸿蒙端暂不接入跨应用划词',
        text: '选取助手需要桌面端的选区浮层能力。鸿蒙版只走输入法扩展，不注册无障碍服务，因此这里不做假的开关。',
        tone: 'warning'
      })

      if (this.error.length > 0) {
        Surface({ tone: 'alt' }) {
          Text(this.error)
            .fontSize(Type.label)
            .fontColor(Palette.danger())
            .width('100%')
        }
      }

      Group() {
        Column() {
          RowItem({
            symbol: 'translation',
            accentSymbol: true,
            title: '翻译选中的内容',
            subtitle: '在翻译页粘贴文字后按目标语言改写',
            showChevron: true,
            showDivider: true,
            onTap: () => Nav.go(Nav.TRANSLATION)
          })
          RowItem({
            symbol: 'qa',
            accentSymbol: true,
            title: '对内容提问',
            subtitle: '把选中文字放进问答页，用语音继续追问',
            showChevron: true,
            showDivider: true,
            onTap: () => Nav.go(Nav.QA)
          })
          RowItem({
            symbol: 'mic',
            accentSymbol: true,
            title: '直接口述改写',
            subtitle: '回到主页或历史页用听写完成',
            showChevron: true,
            showDivider: false,
            onTap: () => Nav.go(Nav.OVERVIEW)
          })
        }
        .width('100%')
      }

      Surface({ tone: 'alt' }) {
        Text('扩展思路：等鸿蒙侧提供选区感知能力（或桌面版共享剪贴板扩展）后，这里再接真正的划词浮层。')
          .fontSize(Type.label)
          .lineHeight(Leading.body)
          .fontColor(Palette.text2())
          .width('100%')
      }
    }
  }
}
