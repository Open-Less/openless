# 旧 Qwen-ASR 子模块清单（已下架）

这份清单已被 [macOS 实验性本地语音模型](experimental-local-asr.md) 取代，不再是当前构建指南。

旧 `qwen-asr`、`qwen3-asr-rs` 子模块、原生 C/MLX 引擎、Metal 资源打包与对应 CI 启用/禁用脚本均已从应用移除。**不要为解决当前编译问题恢复旧子模块或旧 MLX 打包步骤。**

TextStream 使用独立、按需安装的 Python/MLX worker。升级它时核对固定 revision、模型清单、wheel 版本/哈希及许可，并运行流式协议、真实推理、取消/退出和安装包验收；不再更新原生子模块指针。
