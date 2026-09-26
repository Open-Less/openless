# 内置的 Wayland 协议 XML

`ext-data-control-v1.xml` 是从 wayland-protocols **1.49** 的
`staging/ext-data-control/ext-data-control-v1.xml` **逐字节复制**过来的（MIT，
版权头保留在文件里）。md5 `dbce6a71086f0a2eb675db5b84d9fd43`。

为什么内置而不是用系统那份：

- 插件要自己看合成器上的 PRIMARY 选区（原因见 `../primary_selection.h`），
  这需要 `ext_data_control_manager_v1` 的客户端胶水代码；
- 胶水由 `wayland-scanner` 在构建时从 XML 生成。若依赖系统
  `/usr/share/wayland-protocols/`，则**构建机必须装 wayland-protocols**，
  而打包路径（`app/scripts/package-linux-egui.sh` 直接取 `build/libopenless.so`）
  并不保证这一点；
- 内置后构建只需要 `wayland-scanner` + `wayland-client` 头/库（CMake 会给出
  明确的缺失报错）。

升级方式：用新版 wayland-protocols 的同名文件替换，并在上面更新版本号与 md5，
然后跑 `../build.sh` 与两个契约用例。
