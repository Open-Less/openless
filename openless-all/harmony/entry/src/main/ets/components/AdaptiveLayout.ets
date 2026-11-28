import { Layout, Space } from '../common/Theme';

/**
 * Reflow the same slots rather than switching two page trees on rotation.
 * Builders use arrow wrappers to preserve their owning page's `this`.
 * https://developer.huawei.com/consumer/cn/doc/harmonyos-guides/arkts-builderparam
 * Rotation uses the platform's synchronized layout animation, not a second spring.
 */
@Component
export struct AdaptiveColumns {
  @Prop bp: string = Layout.SM;
  @Prop secondaryWidth: number = 320;
  @Prop singleColumn: boolean = false;
  @BuilderParam primary: () => void;
  @BuilderParam secondary: () => void;

  private wide(): boolean {
    return !this.singleColumn && Layout.isWide(this.bp);
  }

  build() {
    Flex({ direction: this.wide() ? FlexDirection.Row : FlexDirection.Column,
      alignItems: ItemAlign.Start }) {
      Column({ space: Space.lg }) {
        this.primary()
      }
      .width(this.wide() ? undefined : '100%')
      .layoutWeight(this.wide() ? 1 : 0)
      .alignItems(HorizontalAlign.Start)

      Column({ space: Space.lg }) {
        this.secondary()
      }
      .width(this.wide() ? this.secondaryWidth : '100%')
      .margin({ left: this.wide() ? Layout.columnGap(this.bp) : 0,
        top: this.wide() ? 0 : Space.lg })
      .alignItems(HorizontalAlign.Start)
    }
    .width('100%')
  }
}
