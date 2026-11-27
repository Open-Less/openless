# 第三方组件与许可证

OpenLess 本体以 **AGPL-3.0** 发布（见仓库 LICENSE）。Linux Qt 宿主用到的第三方库如下，
均为**动态链接**，未作任何修改。

| 组件 | 许可证 | 说明 |
|---|---|---|
| Qt 6（Core/Gui/Widgets/Network） | LGPL-3.0 或 GPL-2.0（Qt 官方双许可以外的第三种为商业授权） | 动态链接系统库 |
| Qt WebEngine（含 Chromium） | LGPL-3.0（Chromium 部分为 BSD 系） | 动态链接系统库 |
| LayerShellQt（KDE Frameworks） | LGPL-3.0-or-later，附 KDE e.V. 例外 | 动态链接系统库 |
| fcitx5 及插件 | LGPL-2.1-or-later | 本仓库自写的插件见 scripts/linux-fcitx5-plugin |

## LGPLv3 合规要点

1. **动态链接**：不静态链接 Qt；发行包只声明 `Depends`，用户系统提供 `.so`，
   因此用户可自行替换 Qt 版本（LGPLv3 §4 的组合要求得到满足）。
2. **许可证文本**：发行包在 `/usr/share/doc/openless/licenses/` 下附带上述组件的
   版权与许可证声明。
3. **未修改上游**：我们未修改 Qt / LayerShellQt 源码；若将来打补丁，补丁本身须按
   LGPLv3（对 Qt）公开。
4. **不禁逆向**：本项目本身即 AGPL-3.0 开源，LGPLv3 允许为调试目的逆向被链接库的
   条款不受限制。
5. **AGPL 与 LGPL 兼容**：GPLv3 §13 明确允许与 AGPLv3 代码组合，故本组合无冲突。
