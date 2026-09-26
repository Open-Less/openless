# macOS CI 与打包耗时

状态：实现说明；更新：2026-09-25。范围仅包含 macOS 检查和桌面打包。Windows、Android、Linux 的编译参数与发布行为由原有工作流维护。

## 调研依据

对照 `beta` 的 `d9113e0b`、[CI 35984434219](https://github.com/Open-Less/openless/actions/runs/35984434219) 与 [桌面构建 35989822601](https://github.com/Open-Less/openless/actions/runs/35989822601) 的完整日志：

| 基线步骤 | 时间 | 日志证据 |
| --- | --- | --- |
| macOS 检查 job | 24 分 59 秒 | stable 检查、测试、MSRV 串行执行 |
| stable `cargo check` | 7 分 54 秒 | 含 MLX，本轮 dev profile 带 debug info |
| stable Tauri 库测试编译 | 7 分 27 秒 | 再次编译 qwen3、Core、Host，test profile 不带 debug info |
| Rust 1.88 Host 检查 | 5 分 03 秒 | 第三次编译 qwen3 与 Host |
| Rust 1.88 独立 backend-tests 编译 | 58.51 秒 | 独立 target 未配置缓存 |
| Apple Silicon release Rust 编译 | 14 分 16 秒 | 依赖缓存命中约 731 MB，仍重新编译 qwen3、Core、Host |
| Intel release Rust 编译 | 13 分 57 秒 | 依赖缓存命中约 700 MB，仍重新编译 Core、Host |

ARM/Intel 的整个打包步骤分别约 15 分 17 秒、15 分 08 秒。主要等待发生在 Rust，而不是 npm 安装、DMG 或 artifact 上传。时间是该次运行的观测值，不是不同 runner、缓存和提交之间的性能保证。

源码中的相关因素：

- `ci.yml` 的 macOS job 先 metadata-only check，再生成测试机器码；dev/test 的 debug 配置也不同。MSRV 与 stable 共用 job 和缓存，Core workspace、独立 backend-tests 的 target 没有全部纳入缓存。
- Tauri release profile 使用 `opt-level=3`、thin LTO 和 `codegen-units=1`。最后一个设置限制单个大 crate 的 LLVM 并行能力；命中第三方依赖缓存仍无法避免 Core/Host 的代码生成成本。
- `build-mac.sh` 曾将所有 `qwen3-asr-rs-*` 目录当成重复输出。Cargo 实际分别保存 build-script 可执行文件和 `OUT_DIR`；只留一个目录会破坏下一轮缓存。
- `rust-cache` 默认只保留依赖产物，不应把命中缓存等同于整个应用无须重编译。参见 [rust-cache 缓存行为](https://github.com/Swatinem/rust-cache#cache-details)。没有添加 sccache：该工具不能缓存调用系统 linker 的 bin/cdylib/proc-macro，参见 [官方限制](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)。

## 当前流程

`ci.yml` 的 macOS stable job 用 `cargo test --lib --bins` 编译并运行库及二进制目标，覆盖库的生产构建与测试构建。MSRV 单独并行执行，继续检查完整 Tauri Host 并编译独立 backend-tests。两个 job 均保留 `--locked`、MLX 子模块与两路编译并发限制；stable 保留全部前端/合同测试和 Codex sandbox 实测。

macOS 检查统一关闭 dev/test debug info，分别缓存 Core、Host、backend-tests 的实际 target 目录。stable、MSRV、release 缓存分开，避免不同工具链和 profile 的产物互相挤占。

MLX 的 CMake 输出另用 [cache-macos-mlx](../.github/actions/cache-macos-mlx/action.yml) 保存。实际使用的 `rust-cache` [源码](https://github.com/Swatinem/rust-cache/blob/6323deb102c322ba6fcbdcafc7e3dddab59af2b6/src/workspace.ts) 排除 workspace 目录内的 path 依赖，导致 `src-tauri/vendor/qwen3-asr-rs` 的原生输出在 post 阶段被清理；首轮验证中，命中 Cargo 缓存仍重建 MLX 约 7 分 17 秒。独立缓存步骤放在 `rust-cache` 后，利用 post 的逆序执行先保存原生输出。缓存按架构、profile、Rust/Clang/Metal/CMake/SDK、子模块提交、编译环境和 manifest/lock/config 隔离，无跨 key 的模糊回退。Metal 版本输出去掉每次启动可能变化的挂载目录，保留实际版本与目标信息。只保存 CMake `out`，不保存 Cargo freshness 指纹，下一轮仍执行 build script 与 CMake 输入校验。

`scripts/macos-build-env.sh` 为 macOS 打包默认设置 `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`，保留 `opt-level=3`、thin LTO 和 unwind；环境变量可以显式覆盖为其他值。参数取舍依据 [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html#codegen-units)：更多 codegen units 允许更快的并行代码生成，可能影响最终体积或优化效果。共享 `Cargo.toml` 不修改。CI 在恢复缓存前加载同一环境，本地 `build-mac.sh` 也加载它。

MLX 清理只比较含非空 metallib 的输出目录，保留 build-script 可执行文件。构建前删除本次架构的旧 app、DMG 和 updater，保留 Cargo 编译缓存；Tauri 非零退出直接失败。因此热构建复用旧时间戳二进制时仍能正确打包，失败时也不会接受旧安装包。现有用途声明、签名、公证和 MLX 包内容校验继续执行。

## 仅 macOS 的验证入口

从待验证分支手动触发已有工作流：

```sh
gh workflow run ci.yml --repo Open-Less/openless --ref <branch> -f platform=macos
gh workflow run release-tauri.yml --repo Open-Less/openless --ref <branch> -f platform=macos
```

前者只运行 macOS stable/MSRV，后者并行生成 Apple Silicon 与 Intel 的桌面包。使用分支 ref，不创建 tag 或 GitHub Release。省略 input 的既有手动运行以及 tag 发布仍使用原有全部平台矩阵。

桌面工作流上传两个架构的 Cargo HTML timings；失败时若已生成计时文件也会上传。DMG/updater 本身已压缩，artifact 使用 `compression-level: 0`。

本地在 `openless-all/app` 运行 `npm test`、`cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --bins` 与 `INSTALL=0 bash scripts/build-mac.sh`。`macos-build-cache.test.mjs` 实际执行隔离的 shell 构建流程，验证缓存目录保留、重复热打包、旧产物清理以及失败传播。

性能验收应记录一次新缓存运行及相同提交的再次运行，分别报告 Rust 编译、完整 job 和产物大小。不同机器的本地构建时间不与 GitHub runner 直接比较；构建通过不等于真实设备 ASR 性能已经验证。
