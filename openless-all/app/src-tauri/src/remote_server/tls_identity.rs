//! A persistent, per-installation CA and a separate short-lived TLS leaf.
//! Phones trust the CA once; changes to LAN or virtual-adapter IPs only reissue
//! the leaf. Never silently replace an unreadable CA: that revokes phone trust.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

const IDENTITY_FILE: &str = "remote-tls-identity-v1.json";
const MAX_IDENTITY_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    ca_cert: Vec<u8>,
    ca_key: Vec<u8>,
    leaf_cert: Vec<u8>,
    leaf_key: Vec<u8>,
}

pub(super) struct TlsIdentity {
    /// Only this public CA certificate is offered for phone installation.
    pub trust_cert: Vec<u8>,
    pub ca_fingerprint_sha256: String,
    pub server_config: Arc<rustls::ServerConfig>,
}

fn key_der(bytes: Vec<u8>) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(bytes))
}

fn matching_key(cert: &[u8], key: &[u8]) -> Result<(), String> {
    rustls::sign::CertifiedKey::from_der(
        vec![CertificateDer::from(cert.to_vec())],
        key_der(key.to_vec()),
        &rustls::crypto::ring::default_provider(),
    )
    .map(|_| ())
    .map_err(|error| format!("remote TLS certificate/key mismatch: {error}"))
}

fn parse_cert(cert: &[u8]) -> Result<CertificateParams, String> {
    CertificateParams::from_ca_cert_der(&CertificateDer::from(cert))
        .map_err(|error| format!("invalid remote TLS certificate: {error}"))
}

fn read_identity(path: &Path) -> Result<Option<StoredIdentity>, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect remote TLS identity: {error}")),
    };
    if !metadata.is_file() || metadata.len() > MAX_IDENTITY_BYTES {
        return Err("remote TLS identity must be a regular file under 64 KiB".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("cannot protect remote TLS identity: {error}"))?;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_IDENTITY_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("cannot read remote TLS identity: {error}"))?;
    if bytes.len() as u64 > MAX_IDENTITY_BYTES {
        return Err("remote TLS identity exceeds 64 KiB".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "remote TLS identity is damaged; restore it from backup instead of replacing phone trust".into())
}

fn save_identity(
    path: &Path,
    identity: &StoredIdentity,
    first_install: bool,
) -> Result<(), String> {
    let directory = path.parent().ok_or("remote TLS directory is unavailable")?;
    let bytes = serde_json::to_vec(identity).map_err(|error| error.to_string())?;
    // tempfile creates private files (0600 on Unix). Windows inherits the user's
    // application-data ACL. Keep the key and certificates in one atomic bundle.
    let mut file = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("cannot create remote TLS identity: {error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|error| format!("cannot write remote TLS identity: {error}"))?;
    if first_install {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    }
    .map_err(|error| format!("cannot persist remote TLS identity: {error}"))?;
    Ok(())
}

fn create_ca(now: OffsetDateTime) -> Result<StoredIdentity, String> {
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "OpenLess Remote Input CA");
    name.push(DnType::OrganizationName, "OpenLess");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(3650);
    let key = KeyPair::generate().map_err(|error| error.to_string())?;
    let cert = params
        .self_signed(&key)
        .map_err(|error| error.to_string())?;
    Ok(StoredIdentity {
        ca_cert: cert.der().to_vec(),
        ca_key: key.serialize_der(),
        leaf_cert: Vec::new(),
        leaf_key: Vec::new(),
    })
}

fn valid_leaf(identity: &StoredIdentity, sans: &[String], now: OffsetDateTime) -> bool {
    let Ok(params) = parse_cert(&identity.leaf_cert) else {
        return false;
    };
    let Ok(required) = CertificateParams::new(sans.to_vec()) else {
        return false;
    };
    if !matches!(params.is_ca, IsCa::NoCa | IsCa::ExplicitNoCa)
        || params.not_before > now
        || params.not_after < now + Duration::days(30)
        || !required
            .subject_alt_names
            .iter()
            .all(|san| params.subject_alt_names.contains(san))
        || matching_key(&identity.leaf_cert, &identity.leaf_key).is_err()
    {
        return false;
    }
    // Validate the cached leaf against the stored trust anchor, not a sidecar
    // list of SANs. A stale or mismatched leaf must never reach the listener.
    use rustls::client::danger::ServerCertVerifier;
    let mut roots = rustls::RootCertStore::empty();
    if roots
        .add(CertificateDer::from(identity.ca_cert.clone()))
        .is_err()
    {
        return false;
    }
    let Ok(verifier) = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build() else {
        return false;
    };
    let Some(name) = sans
        .first()
        .and_then(|name| rustls::pki_types::ServerName::try_from(name.clone()).ok())
    else {
        return false;
    };
    let Ok(timestamp) = u64::try_from(now.unix_timestamp()) else {
        return false;
    };
    verifier
        .verify_server_cert(
            &CertificateDer::from(identity.leaf_cert.as_slice()),
            &[],
            &name,
            &[],
            rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_secs(
                timestamp,
            )),
        )
        .is_ok()
}

pub(super) fn load_or_create(directory: &Path, sans: &[String]) -> Result<TlsIdentity, String> {
    load_at(directory, sans, OffsetDateTime::now_utc())
}

fn load_at(directory: &Path, sans: &[String], now: OffsetDateTime) -> Result<TlsIdentity, String> {
    if sans.is_empty() {
        return Err("remote TLS requires at least one server name".into());
    }
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("cannot create remote TLS directory: {error}"))?;
    let path = directory.join(IDENTITY_FILE);
    let saved = read_identity(&path)?;
    let first_install = saved.is_none();
    let mut identity = match saved {
        Some(identity) => identity,
        None => create_ca(now)?,
    };
    matching_key(&identity.ca_cert, &identity.ca_key)?;
    let (remainder, ca_x509) = x509_parser::parse_x509_certificate(&identity.ca_cert)
        .map_err(|_| "invalid remote TLS CA encoding")?;
    if !remainder.is_empty()
        || ca_x509.issuer() != ca_x509.subject()
        || ca_x509.verify_signature(None).is_err()
    {
        return Err("remote TLS CA signature is invalid; restore the identity from backup".into());
    }
    let ca_params = parse_cert(&identity.ca_cert)?;
    if !matches!(ca_params.is_ca, IsCa::Ca(_))
        || !ca_params.key_usages.contains(&KeyUsagePurpose::KeyCertSign)
        || ca_params.not_before > now
        || ca_params.not_after <= now + Duration::days(30)
    {
        return Err("remote TLS CA is invalid or expires soon; restore a valid identity or explicitly reset it and trust the new certificate on each phone".into());
    }
    if !valid_leaf(&identity, sans, now) {
        let ca_key = KeyPair::try_from(&key_der(identity.ca_key.clone()))
            .map_err(|error| error.to_string())?;
        let ca_expiry = ca_params.not_after;
        let ca = ca_params
            .self_signed(&ca_key)
            .map_err(|error| error.to_string())?;
        let key = KeyPair::generate().map_err(|error| error.to_string())?;
        let mut params =
            CertificateParams::new(sans.to_vec()).map_err(|error| error.to_string())?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, "OpenLess Remote Input");
        name.push(DnType::OrganizationName, "OpenLess");
        params.distinguished_name = name;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = now - Duration::days(1);
        params.not_after = (now + Duration::days(365)).min(ca_expiry);
        params.use_authority_key_identifier_extension = true;
        let cert = params
            .signed_by(&key, &ca, &ca_key)
            .map_err(|error| error.to_string())?;
        identity.leaf_cert = cert.der().to_vec();
        identity.leaf_key = key.serialize_der();
        if !valid_leaf(&identity, sans, now) {
            return Err("generated remote TLS certificate failed validation".into());
        }
        save_identity(&path, &identity, first_install)?;
    }
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| error.to_string())?
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(identity.leaf_cert)],
        key_der(identity.leaf_key),
    )
    .map_err(|error| format!("remote TLS config: {error}"))?;
    Ok(TlsIdentity {
        ca_fingerprint_sha256: fingerprint_sha256(&identity.ca_cert),
        trust_cert: identity.ca_cert,
        server_config: Arc::new(config),
    })
}

/// 对实际 DER 证书计算指纹，而非散列名称或描述文件元数据。
/// 电脑通过本地 IPC 显示完整值，供用户从独立渠道核对。
pub(super) fn fingerprint_sha256(cert: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(cert))
}

/// The profile contains a public CA only. IDs include its fingerprint so two
/// computers can be trusted without replacing each other's iOS profiles.
pub(super) fn mobileconfig(cert: &[u8]) -> String {
    use base64::Engine;
    let fingerprint = fingerprint_sha256(cert);
    let b64 = base64::engine::general_purpose::STANDARD.encode(cert);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>PayloadContent</key><array><dict>
<key>PayloadCertificateFileName</key><string>openless-ca.cer</string>
<key>PayloadContent</key><data>{b64}</data>
<key>PayloadType</key><string>com.apple.security.root</string>
<key>PayloadIdentifier</key><string>com.openless.remote-input.{fingerprint}.cert</string>
<key>PayloadUUID</key><string>{cert_uuid}</string>
<key>PayloadVersion</key><integer>1</integer>
<key>PayloadDisplayName</key><string>OpenLess Remote Input CA ({short})</string>
</dict></array>
<key>PayloadDisplayName</key><string>OpenLess Remote Input ({short})</string>
<key>PayloadDescription</key><string>Before trusting, compare the certificate's full SHA-256 fingerprint in system certificate details with OpenLess settings on your computer. Names and profile identifiers are not proof of identity. Expect exactly one root certificate and no other settings. If the fingerprint differs or cannot be checked, do not install or enable full trust; remove the profile if already installed. This CA can issue certificates; remove it when no longer needed.</string>
<key>PayloadIdentifier</key><string>com.openless.remote-input.{fingerprint}</string>
<key>PayloadType</key><string>Configuration</string>
<key>PayloadUUID</key><string>{profile_uuid}</string>
<key>PayloadVersion</key><integer>1</integer>
</dict></plist>"#,
        short = &fingerprint[..8],
        cert_uuid = uuid::Uuid::new_v4(),
        profile_uuid = uuid::Uuid::new_v4(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        vec!["localhost".into(), "127.0.0.1".into(), "192.168.1.2".into()]
    }

    fn stored(directory: &Path) -> StoredIdentity {
        read_identity(&directory.join(IDENTITY_FILE))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn fingerprint_uses_full_sha256() {
        assert_eq!(
            fingerprint_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn copied_profile_labels_do_not_authenticate_a_replacement_certificate() {
        use base64::Engine;
        let original_dir = tempfile::tempdir().unwrap();
        let replacement_dir = tempfile::tempdir().unwrap();
        let original = load_or_create(original_dir.path(), &names()).unwrap();
        let replacement = load_or_create(replacement_dir.path(), &names()).unwrap();

        // 两张证书的名称完全相同，描述文件的名称和标识也可以照抄。
        assert_eq!(
            parse_cert(&original.trust_cert).unwrap().distinguished_name,
            parse_cert(&replacement.trust_cert).unwrap().distinguished_name
        );
        let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let substituted = mobileconfig(&original.trust_cert).replace(
            &encode(&original.trust_cert),
            &encode(&replacement.trust_cert),
        );
        assert!(substituted.contains(&original.ca_fingerprint_sha256));
        let certificate_data = substituted
            .split_once("<data>")
            .unwrap()
            .1
            .split_once("</data>")
            .unwrap()
            .0;
        let actual_certificate = base64::engine::general_purpose::STANDARD
            .decode(certificate_data)
            .unwrap();

        // 独立查看实际证书会发现指纹不同，且原 CA 的 TLS 验证拒绝替换证书。
        assert_ne!(
            fingerprint_sha256(&actual_certificate),
            original.ca_fingerprint_sha256
        );
        assert_eq!(
            fingerprint_sha256(&actual_certificate),
            replacement.ca_fingerprint_sha256
        );
        assert!(!verify_server(&replacement, &original.trust_cert, "localhost"));
        assert_ne!(
            original.ca_fingerprint_sha256,
            fingerprint_sha256(&stored(original_dir.path()).leaf_cert)
        );
    }

    fn verify_server(identity: &TlsIdentity, trusted_ca: &[u8], name: &str) -> bool {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(trusted_ca.to_vec()))
            .unwrap();
        let client_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let mut client = rustls::ClientConnection::new(
            Arc::new(client_config),
            name.to_owned().try_into().unwrap(),
        )
        .unwrap();
        let mut server =
            rustls::ServerConnection::new(Arc::clone(&identity.server_config)).unwrap();
        for _ in 0..10 {
            let mut bytes = Vec::new();
            client.write_tls(&mut bytes).unwrap();
            server.read_tls(&mut bytes.as_slice()).unwrap();
            if server.process_new_packets().is_err() {
                return false;
            }
            bytes.clear();
            server.write_tls(&mut bytes).unwrap();
            client.read_tls(&mut bytes.as_slice()).unwrap();
            if client.process_new_packets().is_err() {
                return false;
            }
            if !client.is_handshaking() && !server.is_handshaking() {
                return true;
            }
        }
        false
    }

    #[test]
    fn restart_reuses_identity_and_tls_works_with_only_the_downloaded_ca() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), &names()).unwrap();
        let before = std::fs::read(dir.path().join(IDENTITY_FILE)).unwrap();
        let second = load_or_create(dir.path(), &names()).unwrap();
        assert_eq!(
            before,
            std::fs::read(dir.path().join(IDENTITY_FILE)).unwrap()
        );
        assert_eq!(first.trust_cert, second.trust_cert);
        assert_eq!(
            first.ca_fingerprint_sha256,
            fingerprint_sha256(&first.trust_cert)
        );
        assert_eq!(first.ca_fingerprint_sha256, second.ca_fingerprint_sha256);
        for name in names() {
            assert!(verify_server(&second, &first.trust_cert, &name));
        }
        assert!(!verify_server(&second, &first.trust_cert, "192.168.1.99"));
        let saved = stored(dir.path());
        let ca = parse_cert(&saved.ca_cert).unwrap();
        let leaf = parse_cert(&saved.leaf_cert).unwrap();
        assert!(matches!(
            ca.is_ca,
            IsCa::Ca(BasicConstraints::Constrained(0))
        ));
        assert!(matches!(leaf.is_ca, IsCa::NoCa | IsCa::ExplicitNoCa));
        assert!(leaf
            .extended_key_usages
            .contains(&ExtendedKeyUsagePurpose::ServerAuth));
        assert!(leaf.not_after - leaf.not_before <= Duration::days(366));
    }

    #[test]
    fn virtual_adapter_and_lan_changes_preserve_phone_trust() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), &names()).unwrap();
        let before = stored(dir.path());
        let mut changed = names();
        changed.push("172.26.112.1".into());
        changed.push("192.168.2.3".into());
        let second = load_or_create(dir.path(), &changed).unwrap();
        let after = stored(dir.path());
        assert_eq!(before.ca_cert, after.ca_cert);
        assert_eq!(before.ca_key, after.ca_key);
        assert_eq!(first.ca_fingerprint_sha256, second.ca_fingerprint_sha256);
        assert_ne!(before.leaf_cert, after.leaf_cert);
        assert!(verify_server(&second, &first.trust_cert, "192.168.2.3"));
        // Removing an adapter does not require another leaf or CA.
        load_or_create(dir.path(), &names()).unwrap();
        assert_eq!(after.leaf_cert, stored(dir.path()).leaf_cert);
    }

    #[test]
    fn renews_expiring_leaf_without_rotating_ca() {
        let dir = tempfile::tempdir().unwrap();
        let now = OffsetDateTime::now_utc();
        load_at(dir.path(), &names(), now).unwrap();
        let before = stored(dir.path());
        load_at(dir.path(), &names(), now + Duration::days(340)).unwrap();
        let after = stored(dir.path());
        assert_eq!(before.ca_cert, after.ca_cert);
        assert_ne!(before.leaf_cert, after.leaf_cert);
        assert!(parse_cert(&after.leaf_cert).unwrap().not_after > now + Duration::days(700));
    }

    #[test]
    fn repairs_bad_leaf_but_never_replaces_damaged_or_mismatched_ca() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), &names()).unwrap();
        let path = dir.path().join(IDENTITY_FILE);
        let mut saved = stored(dir.path());
        saved.leaf_cert = vec![0];
        save_identity(&path, &saved, false).unwrap();
        let repaired = load_or_create(dir.path(), &names()).unwrap();
        assert!(verify_server(&repaired, &first.trust_cert, "localhost"));
        saved = stored(dir.path());
        let mut damaged = stored(dir.path());
        *damaged.ca_cert.last_mut().unwrap() ^= 1;
        save_identity(&path, &damaged, false).unwrap();
        assert!(load_or_create(dir.path(), &names()).is_err());
        assert_eq!(damaged.ca_cert, stored(dir.path()).ca_cert);
        saved.ca_key = KeyPair::generate().unwrap().serialize_der();
        save_identity(&path, &saved, false).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(load_or_create(dir.path(), &names()).is_err());
        assert_eq!(before, std::fs::read(&path).unwrap());
        std::fs::write(&path, b"interrupted write").unwrap();
        assert!(load_or_create(dir.path(), &names()).is_err());
        assert_eq!(
            b"interrupted write",
            std::fs::read(&path).unwrap().as_slice()
        );
    }

    #[test]
    fn refuses_expired_ca_instead_of_silently_revoking_trust() {
        let dir = tempfile::tempdir().unwrap();
        let now = OffsetDateTime::now_utc();
        load_at(dir.path(), &names(), now).unwrap();
        let before = std::fs::read(dir.path().join(IDENTITY_FILE)).unwrap();
        assert!(load_at(dir.path(), &names(), now + Duration::days(3651)).is_err());
        assert_eq!(
            before,
            std::fs::read(dir.path().join(IDENTITY_FILE)).unwrap()
        );
    }

    #[test]
    fn migration_keeps_legacy_files_and_profiles_only_contain_public_ca() {
        use base64::Engine;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("remote-cert-v4.der"), b"legacy cert").unwrap();
        std::fs::write(dir.path().join("remote-key-v4.der"), b"legacy key").unwrap();
        let first = load_or_create(dir.path(), &names()).unwrap();
        let profile = mobileconfig(&first.trust_cert);
        let saved = stored(dir.path());
        let encoded = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        assert!(profile.contains(&format!("<data>{}</data>", encoded(&first.trust_cert))));
        assert!(!profile.contains(&encoded(&saved.ca_key)));
        assert!(!profile.contains(&encoded(&saved.leaf_key)));
        assert!(!profile.contains(&encoded(&saved.leaf_cert)));
        assert_eq!(
            std::fs::read(dir.path().join("remote-cert-v4.der")).unwrap(),
            b"legacy cert"
        );
        assert_eq!(
            std::fs::read(dir.path().join("remote-key-v4.der")).unwrap(),
            b"legacy key"
        );
        let other = tempfile::tempdir().unwrap();
        let second = load_or_create(other.path(), &names()).unwrap();
        assert_ne!(first.trust_cert, second.trust_cert);
        assert!(!verify_server(&second, &first.trust_cert, "localhost"));
        assert_ne!(profile, mobileconfig(&second.trust_cert));
    }

    #[test]
    fn persistence_errors_never_fall_back_to_an_ephemeral_identity() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"not a directory").unwrap();
        assert!(load_or_create(&blocked, &names()).is_err());
        std::fs::create_dir(dir.path().join(IDENTITY_FILE)).unwrap();
        assert!(load_or_create(dir.path(), &names()).is_err());
        assert!(load_or_create(dir.path(), &[]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn stored_private_keys_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        load_or_create(dir.path(), &names()).unwrap();
        assert_eq!(
            std::fs::metadata(dir.path().join(IDENTITY_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
