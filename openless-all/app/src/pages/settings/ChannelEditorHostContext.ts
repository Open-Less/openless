import { createContext } from 'react';

/** 设置右侧的渠道编辑容器；引导页不提供此容器，继续使用独立弹窗。 */
export const ChannelEditorHostContext = createContext<{
  /** 子页挂载位置，覆盖右侧内容但保留左侧导航。 */
  container: HTMLDivElement | null;
  /** 被子页覆盖的内容；编辑期间设为 inert，避免键盘焦点落入背后。 */
  background: HTMLDivElement | null;
  /** 切换设置分类或关闭设置时，先完成编辑器的草稿清理。 */
  registerClose: (close: (() => void) | null) => void;
} | null>(null);
