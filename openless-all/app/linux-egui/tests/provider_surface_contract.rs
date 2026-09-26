//! 契约：Linux 界面必须是 Core provider / 凭据模块的**真实客户端**。
//!
//! 这里的每一条对应 CI 门禁 `scripts/check-linux-public-surface.ps1` 的断言。
//! 门禁只为「只有状态、或只有本地模型捷径」的界面留了口子，所以把同一组断言
//! 复制到 Rust 测试里：某次合并或清理把实现覆盖掉时，本机 `cargo test` 就会
//! 直接失败，而不是等到发布流水线第 8 步才发现（历史上有一次正是这样丢的）。

const MAIN: &str = include_str!("../src/main.rs");

/// 每个 token 都对应 Linux 设置页真实发出的 Core 调用。
#[test]
fn linux_settings_page_drives_core_provider_and_credential_operations() {
    for token in [
        // 渠道管理
        "provider_descriptors",
        "list_channels",
        "create_channel",
        "set_channel_enabled",
        "rename_channel",
        "set_channel_provider_type",
        "reorder_channels",
        "set_active_provider",
        // 凭据写入 / 删除
        "set_credential",
        "remove_credential",
    ] {
        assert!(
            MAIN.contains(token),
            "Linux UI 缺少 Core provider 调用 `{token}`（设置页不再是 Core 的真实客户端）"
        );
    }
}

/// 校验与模型枚举走的是 `services().provider`（`ProviderApi`），而不是界面自己
/// 拼 endpoint。这里检查调用形态，避免有人把 `.provider.validate(...)` 换成自造
/// 的 HTTP 路径。
#[test]
fn linux_ui_validates_and_lists_models_through_the_provider_api() {
    assert!(
        MAIN.contains(".provider"),
        "Linux UI 必须通过 Core 的 ProviderApi 校验与列模型"
    );
    assert!(
        MAIN.contains(".validate("),
        "Linux UI 缺少 Core 的 provider.validate 调用"
    );
    assert!(
        MAIN.contains(".list_models("),
        "Linux UI 缺少 Core 的 provider.list_models 调用"
    );
}

/// 界面不得自己拥有服务商默认值（端点 / 模型 / 预设），否则换 provider 只能靠
/// 改界面代码。与门禁禁止 `ASR_PRESETS` / `LLM_PRESETS` / 写死的 `https://…/v1` 同级。
#[test]
fn linux_ui_does_not_own_provider_defaults() {
    for forbidden in ["ASR_PRESETS", "LLM_PRESETS", "OMNI_PRESETS"] {
        assert!(
            !MAIN.contains(forbidden),
            "Linux UI 不得内置服务商预设 `{forbidden}`，默认值属于 Core"
        );
    }
}
