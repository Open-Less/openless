import { createContext, useContext } from 'react';

/**
 * 跳转到「服务 → 本地模型」视图的回调。由拥有视图状态的 ServicesTab 提供，
 * 渠道编辑器等深层界面通过它把用户带到模型管理页；取不到时（如独立测试渲染）
 * 调用方应退化为隐藏跳转入口。
 */
export const LocalModelsNavContext = createContext<(() => void) | null>(null);

export function useLocalModelsNav(): (() => void) | null {
  return useContext(LocalModelsNavContext);
}
