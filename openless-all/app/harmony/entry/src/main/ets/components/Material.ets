import { deviceInfo } from '@kit.BasicServicesKit';
import { uiMaterial } from '@kit.ArkUI';

/**
 * 沉浸光感（Immersive Light Sensing）。
 *
 * HDS 的系统材质：组件背景由 RenderService 按物理光照模型渲染毛玻璃 + 光晕 +
 * 按压弹性反馈，`interactive: true` 时才有按压形变与触点光效。这不是"自己画一层
 * 渐变"能替代的东西——手画的色块只会是一块半透明色，没有折射与随内容变化。
 *
 * 降级：低版本 SDK 或设备不支持时返回 undefined，`.systemGlass(undefined)`
 * 就是关闭材质，组件回落到 `backgroundBlurStyle` 的普通毛玻璃（见 Common.Card）。
 */
export class Glass {
  private static supported: boolean | undefined = undefined;

  static available(): boolean {
    if (Glass.supported !== undefined) {
      return Glass.supported;
    }
    let ok: boolean = false;
    try {
      ok = deviceInfo.apiAvailable('26.0.0');
    } catch (error) {
      ok = false;
    }
    Glass.supported = ok;
    return ok;
  }

  /** 卡片、列表行等内容容器：细材质，带按压反馈。 */
  static surface(): uiMaterial.ImmersiveMaterial | undefined {
    if (!Glass.available()) {
      return undefined;
    }
    try {
      return new uiMaterial.ImmersiveMaterial({
        style: uiMaterial.ImmersiveStyle.THIN,
        interactive: true,
        lightEffect: { color: undefined }
      });
    } catch (error) {
      return undefined;
    }
  }

  /** 导航栏（侧栏 / 底部标签 / 半模态）：厚材质，内容可隐约透过。 */
  static bar(): uiMaterial.ImmersiveMaterial | undefined {
    if (!Glass.available()) {
      return undefined;
    }
    try {
      return new uiMaterial.ImmersiveMaterial({
        style: uiMaterial.ImmersiveStyle.THICK,
        interactive: false,
        lightEffect: { color: undefined }
      });
    } catch (error) {
      return undefined;
    }
  }
}
