use openless_core::credentials::{CredentialKey, CredentialNamespace};
use openless_core::credentials_legacy::decode_legacy_credentials;

#[test]
fn reloading_volcengine_channels_preserves_service_and_separate_credentials() {
    let saved = r#"{"version":2,"providers":{"asr":{
        "plan-channel":{"providerType":"volcengine","volcengineService":"agent_plan","volcengineApiKey":"plan-key"},
        "legacy-channel":{"providerType":"volcengine","authMode":"app_id_token","appKey":"app","accessKey":"token"}
    }}}"#;
    let loaded = decode_legacy_credentials(saved).unwrap();
    let read = |channel: &str, account: &str| {
        let key =
            CredentialKey::new(CredentialNamespace::Asr, Some(channel.into()), account).unwrap();
        loaded
            .secrets
            .iter()
            .find(|(stored, _)| *stored == key)
            .map(|(_, value)| value.expose_secret())
    };
    assert_eq!(
        read("plan-channel", "volcengine.service"),
        Some("agent_plan")
    );
    assert_eq!(read("plan-channel", "volcengine.api_key"), Some("plan-key"));
    assert_eq!(read("legacy-channel", "volcengine.service"), None);
    assert_eq!(
        read("legacy-channel", "volcengine.access_key"),
        Some("token")
    );
}
