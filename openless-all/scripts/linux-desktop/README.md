# Linux 桌面集成

桥协议为 `org.openless.Desktop1` v1，与 Core 业务数据版本独立。X11 直接使用 X11 接口；Wayland 使用桌面组件提供前台窗口、屏幕工作区、焦点恢复、浮窗定位和全局快捷键。

安装包附带 `openless-desktop-install`。在用户会话中运行：

```sh
openless-desktop-install install
openless-desktop-install enable
openless-desktop-install uninstall
```

GNOME 42–44 和 45+ 使用不同模块入口。首次安装 GNOME 扩展后可能需要注销并重新登录，再执行 `enable`。KDE 使用 KWin 脚本与 KGlobalAccel 辅助程序；安装时写入用户的脚本、D-Bus 服务和登录启动项。安装后重启 OpenLess，使当前设置重新注册到桌面桥。

组件只运行在当前用户的会话中，不要求 root，不读取密码输入框。fcitx5 输入目标使用 UUID 与会话票据；桌面窗口身份用于定位与恢复焦点，不代替写入前的文本目标校验。

开发构建：

```sh
node gnome/build.mjs
cmake -S kde -B kde/build -DCMAKE_BUILD_TYPE=Release
cmake --build kde/build --parallel
```

KDE 构建支持 Qt 5 / KF5 与 Qt 6 / KF6。桌面组件的实际交互验收见 Linux 交接清单；编译通过不代表已经在相应桌面版本上完成验收。

接口参考：[GNOME 扩展](https://gjs.guide/extensions/)、[KWin](https://develop.kde.org/docs/plasma/kwin/api/)、[KGlobalAccel](https://api.kde.org/kglobalaccel.html)、[AT-SPI Text](https://gnome.pages.gitlab.gnome.org/at-spi2-core/libatspi/iface.Text.html)。
