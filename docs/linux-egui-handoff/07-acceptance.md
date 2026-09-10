# 07：验收和证据

## 1. 两个不同的完成门

- **本批Core移交门**：可调用的`2.0.0`合同、真实共享业务、平台Interface、无设备示例/fixture、跨平台依赖检查和逐项缺口文档。
- **egui团队Linux产品门**：补齐[登记项](./02-gap-register.md)，取得真实桌面、设备、安装升级与正式分发证据。

第二项不阻塞本批Windows/macOS 2.0；共享Core缺陷或不可调用的承诺接口仍属于本批问题。
Windows/macOS自己的完整功能与设备验收见[桌面清单](../2.0-desktop-acceptance.md)。

## 2. 无设备验证

在仓库`openless-all/app`目录执行；Linux原生依赖安装方式沿用[CI](../../.github/workflows/ci.yml)，不要把Windows上的Linux stub编译当作Linux目标验证。

```sh
cargo test -p openless-core --locked
cargo test -p openless-linux-egui --locked
cargo run -p openless-linux-egui --example headless_host --locked
cargo check -p openless-linux-egui --all-targets --locked
pwsh -NoProfile -File "scripts/check-core-deps.ps1"
pwsh -NoProfile -File "scripts/check-core-deps.ps1" -Package openless-linux-egui
pwsh -NoProfile -File "scripts/check-core-secret-surface.ps1"
pwsh -NoProfile -File "scripts/check-core-test-isolation.ps1"
pwsh -NoProfile -File "scripts/check-core-runtime-seam.ps1"
pwsh -NoProfile -File "scripts/check-linux-public-surface.ps1"
```

headless示例使用fixture，不证明音频/fcitx5/Secret Service可用。上述命令是接手方可复用的验证入口，不表示本次文档更新重新执行过测试。

插件从仓库根目录构建，在已安装Fcitx5开发依赖的Linux环境执行；build目录使用独立路径：

```sh
cmake -S "openless-all/scripts/linux-fcitx5-plugin" -B "build/linux-fcitx5-contract" -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
cmake --build "build/linux-fcitx5-contract"
ctest --test-dir "build/linux-fcitx5-contract" --output-on-failure
```

这验证C++目标合同；不替代真实前台应用输入。保留发行版最低Fcitx版本编译覆盖，避免只在较新本机头文件通过。

## 3. 必须显式执行的环境测试

返回仓库的`openless-all/app`目录。以下测试默认ignored，只有在适当的测试账号、运行中服务和设备上才执行：

```sh
cargo test -p openless-linux-egui --test cpal_contract -- --ignored
cargo test -p openless-linux-egui --test fcitx5_contract -- --ignored
OPENLESS_RUN_SECRET_SERVICE_CONTRACT=1 cargo test -p openless-linux-egui --test secret_service_contract -- --ignored
```

以上为Linux shell命令。Secret Service须在已启动测试keyring的DBus会话中执行；其显式环境开关只授权该次测试。
先阅读相应[测试文件](../../openless-all/app/linux-egui/tests)：fcitx5测试会修改热键并尝试向焦点目标提交文本，必须使用隔离桌面/测试编辑器，结束后恢复配置；keyring测试会写入并移除测试凭据。
CPAL测试允许明确分类的无设备/权限错误，fcitx5部分调用也允许不可用错误；测试绿色不等于录到音频或成功落字，仍需下面的真机流程。不要导出真实密钥。

| 环境/领域 | 必须记录的实际结果 |
| --- | --- |
| X11与Wayland，声明支持的桌面环境 | 安装/加载插件、后台全局热键、重绑/冲突/重载、主窗与托盘行为；分别声明限制 |
| GTK/Qt/浏览器/终端 | 普通/流式Unicode落字、换行、切焦点、目标失效、选区确认/撤回、未知结果不双写 |
| 音频 | 真麦克风、切换/拔出、静音恢复、录音archive/重试/重转、Starting至finish全过程取消 |
| Provider/vault/model | 锁定与解锁、保存失败恢复、旧数据幂等迁移、真实云请求、Qwen推理与取消/释放 |
| QA/Selection Voice/Agent | 多轮、意图/预览、审批/拒绝、旧事件隔离、关闭重开、CLI stdin与进程树清理 |
| Remote | 真手机LAN/TLS、PIN更新、首帧、并发隔离、断连/重连、端口冲突与stop后cancel |
| deb/rpm/AppImage | 内容/依赖、安装/启动、插件路径、1.x升级数据、卸载/回滚、权限和签名/更新 |

仅包内文件存在、`--help`成功或CI绿色，不能证明上述完整用户流程。

## 4. Linux分发与回报

已有[Linux打包脚本](../../openless-all/app/scripts/package-linux-egui.sh)和[独立工作流](../../.github/workflows/release-linux-egui.yml)供复用。
无正式签名、manual URL或调试签名的制品按验证产物记录，不宣传为正式更新源；只有管理员可按[发布政策](../../RELEASING.md)发布。

每项关闭记录：`L编号 / commit / OS与桌面版本 / 前提 / 操作 / 期望 / 实际 / 自动日志 / 设备证据 / 仍有限制`。
接口不足附最小复现、期望Core方法/事件/错误回报Core负责人；平台实现或UI缺口由egui团队继续负责。
