//! 凭据账号 → 命名空间的映射（两个宿主共用）。
//!
//! 这套表原来只住在 Tauri 命令层（`src-tauri/src/commands/credentials.rs`），
//! 于是 Linux 的 Web 宿主想读/写凭据就得抄一份 —— 抄两份必然漂移，而"凭据读错
//! key"是静默错误。这里把它下沉到 core，宿主只调用不复制。
use crate::credentials::{CredentialKey, CredentialNamespace};
use crate::{BackendError, BackendErrorCode};

/// GitHub OAuth token 的账号名（Marketplace 用它读写）。
pub const MARKETPLACE_GITHUB_TOKEN_ACCOUNT: &str = "github.oauth_token";

/// 稳定的账号标识。名字与 Swift / Tauri 侧一致，老 Keychain 条目仍可读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialAccount {
    VolcengineAppKey,
    VolcengineAccessKey,
    VolcengineResourceId,
    VolcengineService,
    VolcengineAuthMode,
    VolcengineApiKey,
    ArkApiKey,
    ArkModelId,
    ArkEndpoint,
    AsrApiKey,
    AsrEndpoint,
    AsrModel,
    AsrVocabularyId,
    AsrAdvancedConfig,
    XfyunAppId,
    XfyunApiKey,
    TencentCloudAppId,
    TencentCloudSecretId,
    TencentCloudSecretKey,
    OmniApiKey,
    OmniEndpoint,
    OmniModel,
}

impl CredentialAccount {
    pub fn keyring_account(self) -> &'static str {
        match self {
            Self::VolcengineAppKey => "volcengine.app_key",
            Self::VolcengineAccessKey => "volcengine.access_key",
            Self::VolcengineResourceId => "volcengine.resource_id",
            Self::VolcengineService => "volcengine.service",
            Self::VolcengineAuthMode => "volcengine.auth_mode",
            Self::VolcengineApiKey => "volcengine.api_key",
            Self::ArkApiKey => "ark.api_key",
            Self::ArkModelId => "ark.model_id",
            Self::ArkEndpoint => "ark.endpoint",
            Self::AsrApiKey => "asr.api_key",
            Self::AsrEndpoint => "asr.endpoint",
            Self::AsrModel => "asr.model",
            Self::AsrVocabularyId => "asr.vocabulary_id",
            Self::AsrAdvancedConfig => "asr.advanced_config",
            Self::XfyunAppId => "xfyun.app_id",
            Self::XfyunApiKey => "xfyun.api_key",
            Self::TencentCloudAppId => "tencent_cloud.app_id",
            Self::TencentCloudSecretId => "tencent_cloud.secret_id",
            Self::TencentCloudSecretKey => "tencent_cloud.secret_key",
            Self::OmniApiKey => "omni.api_key",
            Self::OmniEndpoint => "omni.endpoint",
            Self::OmniModel => "omni.model",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "volcengine.app_key" => Self::VolcengineAppKey,
            "volcengine.access_key" => Self::VolcengineAccessKey,
            "volcengine.resource_id" => Self::VolcengineResourceId,
            "volcengine.service" => Self::VolcengineService,
            "volcengine.auth_mode" => Self::VolcengineAuthMode,
            "volcengine.api_key" => Self::VolcengineApiKey,
            "ark.api_key" => Self::ArkApiKey,
            "ark.model_id" => Self::ArkModelId,
            "ark.endpoint" => Self::ArkEndpoint,
            "asr.api_key" => Self::AsrApiKey,
            "asr.endpoint" => Self::AsrEndpoint,
            "asr.model" => Self::AsrModel,
            "asr.vocabulary_id" => Self::AsrVocabularyId,
            "asr.advanced_config" => Self::AsrAdvancedConfig,
            "xfyun.app_id" => Self::XfyunAppId,
            "xfyun.api_key" => Self::XfyunApiKey,
            "tencent_cloud.app_id" => Self::TencentCloudAppId,
            "tencent_cloud.secret_id" => Self::TencentCloudSecretId,
            "tencent_cloud.secret_key" => Self::TencentCloudSecretKey,
            "omni.api_key" => Self::OmniApiKey,
            "omni.endpoint" => Self::OmniEndpoint,
            "omni.model" => Self::OmniModel,
            _ => return None,
        })
    }

    /// 该账号属于哪个 provider 家族 —— 决定显式 provider id 路由到哪个命名空间。
    fn namespace(self) -> CredentialNamespace {
        match self {
            Self::ArkApiKey | Self::ArkModelId | Self::ArkEndpoint => CredentialNamespace::Llm,
            Self::OmniApiKey | Self::OmniEndpoint | Self::OmniModel => CredentialNamespace::Omni,
            _ => CredentialNamespace::Asr,
        }
    }
}

/// 账号名 → 凭据 key（含命名空间与可选 provider）。
///
/// 映射规则（与 Tauri 侧完全一致）：
/// 1. LLM 协议的配置账号（`llm_protocol::CONFIG_ACCOUNTS`）→ `Llm`；
/// 2. LLM 的额外 header / temperature 账号 → `Llm`；Omni 的同名账号 → `Omni`；
/// 3. Marketplace 的 GitHub token → `Marketplace`；云同步自建 token → `Application`；
/// 4. 其余按 `CredentialAccount::namespace()`。
pub fn credential_key(
    account: &str,
    provider: Option<String>,
) -> Result<CredentialKey, BackendError> {
    // 1+2 都是 `Llm`，合成一个条件：clippy 会把两个相同分支报成 identical blocks。
    let namespace = if crate::llm_protocol::CONFIG_ACCOUNTS.contains(&account)
        || account == crate::credentials::LLM_EXTRA_HEADERS_ACCOUNT
        || account == crate::credentials::LLM_TEMPERATURE_ACCOUNT
    {
        CredentialNamespace::Llm
    } else if account == crate::credentials::OMNI_EXTRA_HEADERS_ACCOUNT
        || account == crate::credentials::OMNI_TEMPERATURE_ACCOUNT
    {
        CredentialNamespace::Omni
    } else if account == MARKETPLACE_GITHUB_TOKEN_ACCOUNT {
        CredentialNamespace::Marketplace
    } else if account == crate::credentials::CLOUD_SYNC_CUSTOM_TOKEN_ACCOUNT {
        CredentialNamespace::Application
    } else {
        CredentialAccount::parse(account)
            .ok_or_else(|| {
                BackendError::new(
                    BackendErrorCode::InvalidArgument,
                    format!("unknown credential account: {account}"),
                )
            })?
            .namespace()
    };
    CredentialKey::new(namespace, provider, account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_round_trip() {
        for account in [
            "volcengine.app_key",
            "ark.api_key",
            "asr.api_key",
            "xfyun.app_id",
            "tencent_cloud.secret_key",
            "omni.model",
        ] {
            let parsed = CredentialAccount::parse(account).expect(account);
            assert_eq!(parsed.keyring_account(), account);
        }
    }

    #[test]
    fn namespaces_follow_the_account_family() {
        let llm = credential_key("ark.api_key", None).unwrap();
        let asr = credential_key("asr.api_key", None).unwrap();
        let omni = credential_key("omni.api_key", None).unwrap();
        assert_eq!(llm.namespace, CredentialNamespace::Llm);
        assert_eq!(asr.namespace, CredentialNamespace::Asr);
        assert_eq!(omni.namespace, CredentialNamespace::Omni);
    }

    #[test]
    fn unknown_accounts_are_rejected() {
        assert!(credential_key("nope.nothing", None).is_err());
    }
}
