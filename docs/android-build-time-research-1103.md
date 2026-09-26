# Android APK 编译耗时调研（#1103）

调研日期：2026-09-25。基线：上游 beta `d9113e0b03c0b5998079d5f43dcb33fb8bba50e7`。原始调研与后续实现记录；Beta 3 整合时采用 ABI 并行与缓存回写方案。

## 实测证据

- [tag run 35989822568](https://github.com/Open-Less/openless/actions/runs/35989822568)：job 32 分 32 秒；Build Android release APK 30 分 31 秒。
- Cargo 完成记录为 6m01s、2m14s、5m11s、5m27s、5m37s。前两轮都为 aarch64，第二轮重新编译应用及部分 Tauri crates。重复编译事实已确认，失效原因尚未确认。
- [同 SHA 的 beta dispatch 35985550453](https://github.com/Open-Less/openless/actions/runs/35985550453)也有五轮编译；arm64 第二轮 2m15s。
- tag Gradle 日志明确 cache-read-only=true；仓库默认分支为 beta，beta dispatch 为 false。不能用 post 为零秒单独证明清盘阻止保存。
- beta dispatch 在清盘后仍保存约 171 MB Rust cache，随后 tag 命中同一缓存。这不证明 target 编译产物被保留：workflow 明确提前删除 target、Cargo registry/git 和 Gradle caches。
- 独立前端构建约 15 秒；Tauri beforeBuildCommand 随后再次执行 npm run build。

## 建议顺序

1. 修复清盘与缓存生命周期，并更换缓存版本键，避免继续命中已有残缺缓存；验证连续两次运行的实际缓存内容及编译耗时。post action 在普通 steps 之后执行，仅把清盘移动到普通 steps 末尾无效。
2. 日常 dispatch 可选 ABI，默认 arm64；正式 tag 保留全 ABI。进一步使用 ABI matrix 降低全量墙钟，分别衡量总 runner 分钟和排队时间；发布资产集中汇总，校验 ABI 完整性。
3. 记录 Cargo timings/fingerprint 和两轮 arm64 调用参数、环境、生成文件差异，定位重复编译。不要直接绕过 Tauri/Gradle native 构建。
4. 当前 release 使用 opt-level=3、thin LTO、codegen-units=1。试验仅 dispatch 的快速 profile：关闭 LTO、提高 codegen-units，同时保持签名；单独比较 APK 大小、运行表现和耗时。正式 tag 优化配置暂不改变。
5. ci-disable-macos-qwen3.mjs 每次删除平台依赖后 cargo generate-lockfile。应研究保留已锁定版本的确定性处理，避免无关依赖升级及缓存键变化；stable Rust 也应考虑固定版本并有计划升级。
6. Cargo timings 若表明依赖或主 crate 占比高，再审计 Android 不使用的依赖/features、缩小重编译边界。现有 openless-core 可作为评估起点，不能未经测量就大改架构。
7. 次要优化：删除重复前端构建，验证 Gradle task-output cache，更强的 x64 runner、预置工具链环境，以及 artifact 压缩参数。现有四份上传合计约 14 秒，优先级低。

## 限制与官方资料

- [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)：LTO/codegen-units 是构建速度与产物表现之间的取舍，不能承诺未经实测的收益。
- [rust-cache](https://github.com/Swatinem/rust-cache)：默认排除 workspace crates、清理 incremental；缓存命中不等于应用无需重编译。
- [sccache Rust](https://github.com/mozilla/sccache/blob/main/docs/Rust.md)：不能缓存涉及系统链接的 cdylib 等输出，因此不是 Tauri 主库的万能缓存；可单独评估依赖缓存收益。
- [Gradle Actions v4](https://github.com/gradle/actions/blob/v4/docs/setup-gradle.md)与[GitHub cache scope](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching)：默认分支写缓存与 ref 可见性应一并设计。
- [Gradle build cache](https://docs.gradle.org/current/userguide/build_cache.html)：任务输出缓存和依赖缓存不同；仅缓存输入输出定义完整的任务。
- [Android NDK host](https://developer.android.com/ndk/guides/other_build_systems)：Linux 官方 NDK 使用 x86_64 host 工具链，不能因为目标 APK 是 arm64 就直接换 Linux arm64 runner。
- [Tauri CLI](https://v2.tauri.app/reference/cli/)：Android build 会执行 beforeBuildCommand。

下述基准是优化前的测量；实际加速效果须以整合后上游仓库的完整运行统计为准。基准应覆盖冷缓存、相同依赖下的源码改动、依赖变更，以及正式全 ABI 构建；同时检查签名、APK ABI 和 updater manifest 完整性。

## 实现备注（#1103 跟进）

重复 aarch64 根因已定位：Tauri CLI `android build` 在 `apk::build` 之前会对**第一个** target 调用 `first_target.build(...)`（注释为 initialize plugins），随后 Gradle 再对每个 ABI 执行 `tauri android android-studio-script`。因此首个 ABI 必然两次 cargo；第二轮约 2m 是因为 `write_options` / `inject_resources` 发生在首次编译之后，指纹变脏。不要绕过 Gradle/native 路径；用单 ABI dispatch + 全 ABI matrix 并行降低墙钟。


## Beta 3 发布整合

- 正式 tag 仍使用原来的 release profile，四 ABI 并行构建后统一签名与生成 Beta updater manifest；快速 profile 只允许手动验证运行。
- Gradle 缓存回写前保留编译缓存，但生成的 Kotlin DSL 只读取环境变量，不写入签名密码或 alias。实际 release build 步骤注入凭据；keystore 权限保持 0600。
- APK 收集器使用 runner 既有 Python 3 标准库校验 ZIP 和 CRC，并检查完整 ABI 目录集合；损坏、未知 ABI、多 ABI、重复或缺失产物均有回归测试。
- 未合入 #1106 中额外的 Linux egui 测试修补；本轮没有 Linux 代码或发布工作流变更。
