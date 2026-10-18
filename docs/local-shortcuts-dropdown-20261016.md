# 本地修复：可清空快捷键与下拉位置

范围：修复当前桌面设置并构建 macOS 本地 App / DMG；不发布正式版、不改标签、不替换正在使用的 App，不修改用户配置或系统授权。

## 实现

- [x] 录音、翻译的 IPC 接受 `null`，清除后取消相应原生监听；其他可配置快捷键保留已有可停用能力，风格包绑定也提供共享清空入口。
- [x] 清空在旧版必填结构中保存为显式空绑定，区别于“首次启动没有字段”；不会在重新加载配置或修改其他设置时恢复默认键。空绑定不参与冲突判断，重新录制的输入仍严格校验。
- [x] 移除录音设置和快捷键页中的“Mac 听写键”选择入口；保留 Fn 录制与旧配置读取兼容性，不擅自改写已有用户绑定。
- [x] 普通 Select、可搜索 Combobox 和快捷键录制弹层统一从控件下方展开；边缘空间不足时仅允许向上避让，不横向翻到另一侧。历史录音操作菜单原本就在按钮下方，保持不变。
- [x] 百炼协议选择接入同一公共 Select，避免例外使用系统原生下拉。

## 下拉根因

两个公共下拉组件和快捷键录制弹层的 `side` 都固定为 `right`。另有可搜索长列表的首帧尺寸问题：定位器尚未提供 `--available-height` 时，整个限高表达式失效，长列表按未限高尺寸参与定位，即使下方有足够空间也可能先翻到上方。补充尺寸变量回退值，并用纵向 flex 和可收缩滚动列表限制包含搜索栏的整个菜单。

| 位置 | 修改前 | 修改后 |
| --- | --- | --- |
| `src/components/ui/Select.tsx` | 固定右侧 | 下方优先，禁止横向避让 |
| `src/components/ui/SelectLite.tsx` | 可搜索菜单固定右侧 | 与普通菜单一致 |
| `src/components/ShortcutRecorder.tsx` | 核心热键清空按钮禁用，额外 Mac 听写键入口，右侧录制面板 | 可清空，移除重复入口，下方录制面板 |
| `src/components/ui/Select.css` | 首帧缺少定位变量时列表不限高 | 首帧限高，搜索栏与滚动列表共同遵守可用高度 |

## 验证

- [x] 完整前端测试：124 个测试文件通过。
- [x] 正常原生依赖 `cargo test --lib --bins`：580 通过，4 项需外部模型文件的测试明确忽略，无失败。
- [x] 新增 Core 回归：4/4，覆盖清空后序列化重载、修改其他设置不恢复、首次启动默认值、严格录制校验与空绑定不冲突。
- [x] Chromium / WebKit：受影响的 24 个用例均已取得通过结果，覆盖所有可配置快捷键清空、离开设置再进入、重新绑定、录制弹层定位，以及 90% / 100% / 110% 下普通与搜索菜单的下方定位、长列表滚动稳定性。完整一轮为 23 通过 / 1 失败；失败发生在并行构建重写 `dist` 时页面变空，单独重跑 Chromium 该用例通过，未放宽断言。
- [x] 正常 macOS Release App / DMG 构建完成；ARM64、最低 macOS 14.0、版本 `2.0.0+build.20261016`。App 的 ad-hoc 签名、Speech framework 链接、Speech / 麦克风描述均已验证；DMG 完整性和包内签名有效，包内可执行文件与构建 App 完全一致。临时只读挂载已卸载并清理。
- [x] `/Applications/OpenLess.app` 可执行文件仍为原有 SHA-256 `7f0918b3012d273fb1ec61b3a041089c85acf9fcb04f73e5ce933bbf6076bba4`，未安装替换、未重置权限。

## 本地产物

- 便于安装的 DMG：`/Users/trip/Downloads/OpenLess-local-shortcuts-20261016-arm64.dmg`（28,977,094 字节）。
- DMG SHA-256：`a116089c7e4634210cb538cb9b08aa43a4e364d2203d9290dc7dde5825cd54fe`。
- 构建 App：`openless-all/app/src-tauri/target/release/bundle/macos/OpenLess.app`。
- 新可执行文件 SHA-256：`e6e009cb557e36d691803d1915efad5e1a7995fd5aedd2ca41e53c0480f5fad7`。
- 构建清单：`/Users/trip/Library/Caches/pi-openless-release-20261016/local-shortcuts-build.json`；明确记录这是未提交工作树的本地构建，不代表正式发布。

详细构建日志和验证结果保存在 `/Users/trip/Library/Caches/pi-openless-release-20261016/shortcut-*.log`。未执行新 App 的安装后设备验收；现有应用、设置和授权保持不变。原有根目录未跟踪的 `package.json`、`pnpm-lock.yaml` 未修改。
