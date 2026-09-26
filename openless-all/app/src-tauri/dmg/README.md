# Apple Silicon 安装界面

背景为本次 OpenLess 安装界面生成的摄影风格图像，不包含模拟的应用或文件夹图标。构图是浅色海岸、清晨日光及中央留白，标题、双语安装说明和箭头位于背景中；真正的 `OpenLess.app` 和 `Applications` 别名由 Finder 展示，支持原生拖动安装。

- `installer-background@2x.png`：1536 × 1024，生成图像的交付资源。
- `installer-background.tiff`：包含 768 × 512（72 dpi）和 1536 × 1024（144 dpi）两种表示，供 Finder Retina 背景使用。
- `layout.json`：真实图标 128 pt、文字 14 pt；窗口及图标位置以 `../tauri.macos-mlx.conf.json` 为准。

通过 `INSTALL=0 ./scripts/build-mac.sh` 打包。ARM 构建使用 `scripts/macos-dmg-layout.py` 在 Tauri 压缩和签名之前写入 Finder 布局，不依赖 CI 上的 Finder 或 AppleScript。最终镜像必须通过 helper 的 `verify` 检查；图像只用于安装盘，不进入应用运行时。

设计来源：2026-09-26 用户给出的双图标安装示例及新照片背景要求，由 OpenAI 图像生成工具制作。后续生成可沿用“浅色石灰岩海岸、清晨柔光、中央为真实图标保留空间”的方向；这段说明是设计摘要，不是原始逐字提示。
