// 保守排版：除首页、顶栏、底栏与胶囊窗外，内容区强制单列满宽。
// 通过 html[data-ol-conservative-layout] + global.css 工具类生效。

export function applyConservativeLayout(enabled: boolean): void {
  if (enabled) {
    document.documentElement.dataset.olConservativeLayout = 'true';
  } else {
    delete document.documentElement.dataset.olConservativeLayout;
  }
}

export function isConservativeLayoutActive(): boolean {
  return document.documentElement.dataset.olConservativeLayout === 'true';
}
