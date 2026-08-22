// 易读布局：小屏或大字号时强制同行控件换行，避免横向溢出。
// 通过 html[data-ol-stacked-layout] + global.css 工具类生效。

export function applyStackedLayout(enabled: boolean): void {
  if (enabled) {
    document.documentElement.dataset.olStackedLayout = 'true';
  } else {
    delete document.documentElement.dataset.olStackedLayout;
  }
}

export function applyStackedLayoutFromPrefs(
  stackedRowLayout?: boolean,
): void {
  applyStackedLayout(stackedRowLayout === true);
}

export function isStackedLayoutActive(): boolean {
  return document.documentElement.dataset.olStackedLayout === 'true';
}
