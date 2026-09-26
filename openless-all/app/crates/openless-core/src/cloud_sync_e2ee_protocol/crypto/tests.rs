use super::*;
use serde_json::Value;

fn vectors() -> Vec<Value> {
    serde_json::from_str::<Value>(include_str!("../fixtures/v1.json")).unwrap()["vectors"]
        .as_array()
        .unwrap()
        .clone()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&s[n..n + 2], 16).unwrap())
        .collect()
}
fn metadata(s: &SnapshotUpload) -> VaultMetadata {
    VaultMetadata {
        protocol_version: 1,
        state: VaultState::Active,
        owner_github_id: s.owner_github_id.clone(),
        revision: s.revision,
        vault_id: Some(s.vault_id.clone()),
        key_id: Some(s.key_id.clone()),
        updated_at: Some("2026-09-23T00:00:00Z".into()),
        payload_schema_version: Some(1),
        ciphertext_bytes: s.decoded_ciphertext().unwrap().len() as u64,
        ciphertext_sha256: Some(s.ciphertext_sha256.clone()),
        last_operation_id: Some(s.operation_id.clone()),
    }
}
fn context(s: &SnapshotUpload) -> EncryptContext {
    EncryptContext {
        owner_github_id: s.owner_github_id.clone(),
        vault_id: s.vault_id.clone(),
        key_id: s.key_id.clone(),
        base_revision: s.base_revision,
        operation_id: s.operation_id.clone(),
        kind: s.kind,
        kdf: s.kdf.clone(),
    }
}
fn test_key(v: &Value) -> DerivedKey {
    DerivedKey::from_secret_bytes(Zeroizing::new(
        unhex(v["derivedKeyHex"].as_str().unwrap())
            .try_into()
            .unwrap(),
    ))
}

#[test]
fn independent_reference_argon2_libsodium_vectors_match_every_byte() {
    for vector in vectors() {
        let snapshot: SnapshotUpload = serde_json::from_value(vector["snapshot"].clone()).unwrap();
        snapshot.validate().unwrap();
        let password = NormalizedPassword::new(SecretInput::new(
            vector["passwordInput"].as_str().unwrap().to_owned(),
        ))
        .unwrap();
        password.validate_new().unwrap();
        assert_eq!(
            password.0.as_bytes(),
            unhex(vector["passwordUtf8Hex"].as_str().unwrap())
        );
        let key = derive_key(&password, &snapshot.kdf).unwrap();
        assert_eq!(
            key.expose_bytes().as_slice(),
            unhex(vector["derivedKeyHex"].as_str().unwrap())
        );
        assert_eq!(
            aad(&snapshot).unwrap(),
            unhex(vector["aadUtf8Hex"].as_str().unwrap())
        );
        let json = unhex(vector["plaintextJsonUtf8Hex"].as_str().unwrap());
        let mut frame = Zeroizing::new(Vec::with_capacity(PAD_BLOCK_BYTES + 16));
        frame.extend_from_slice(&(json.len() as u32).to_be_bytes());
        frame.extend_from_slice(&json);
        let padding_len = PAD_BLOCK_BYTES - frame.len();
        let padding = (0..padding_len)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            padding_len as u64,
            vector["padding"]["length"].as_u64().unwrap()
        );
        assert_eq!(
            Sha256Digest::of(&padding).as_hex(),
            vector["padding"]["sha256"].as_str().unwrap()
        );
        frame.extend_from_slice(&padding);
        let encrypted =
            seal_frame(&context(&snapshot), &key, snapshot.nonce.clone(), frame).unwrap();
        assert_eq!(encrypted.ciphertext, snapshot.ciphertext);
        assert_eq!(encrypted.ciphertext_sha256, snapshot.ciphertext_sha256);
        let recovered = decrypt_snapshot(
            &snapshot,
            &metadata(&snapshot),
            &snapshot.owner_github_id,
            &key,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&recovered).unwrap(),
            vector["recoveredJson"]
        );
    }
}

#[test]
fn nfc_confirmation_preserves_spaces_and_applies_local_weak_password_policy() {
    assert!(NormalizedPassword::confirmed_new(
        SecretInput::new("云同步Cafe\u{301}-Vector2026!".into()),
        SecretInput::new("云同步Café-Vector2026!".into())
    )
    .is_ok());
    assert!(NormalizedPassword::confirmed_new(
        SecretInput::new(" Good-PasswordA9! ".into()),
        SecretInput::new("Good-PasswordA9!".into())
    )
    .is_err());
    for input in [
        "12345678",
        "Password123",
        "Password1234",
        "Abc123456789",
        "Qwerty123456",
        "Aa1Aa1Aa1Aa1",
        "P@ssw0rd12345",
    ] {
        assert!(
            NormalizedPassword::new(SecretInput::new(input.into()))
                .and_then(|v| v.validate_new())
                .is_err(),
            "weak password was accepted"
        );
    }
    assert!(NormalizedPassword::new(SecretInput::new(format!("Ab1{}", "中".repeat(126)))).is_err());
}

#[test]
fn identity_revision_kdf_and_aead_tampering_are_rejected() {
    let vector = &vectors()[0];
    let snapshot: SnapshotUpload = serde_json::from_value(vector["snapshot"].clone()).unwrap();
    let meta = metadata(&snapshot);
    let key = test_key(vector);
    let mut bad = snapshot.clone();
    bad.owner_github_id = GithubId::parse("54321").unwrap();
    assert!(matches!(
        decrypt_snapshot(&bad, &meta, &snapshot.owner_github_id, &key),
        Err(Error::AccountMismatch)
    ));
    let mut bad = snapshot.clone();
    bad.revision = Revision::new(1);
    assert!(decrypt_snapshot(&bad, &meta, &snapshot.owner_github_id, &key).is_err());
    let mut bad = snapshot.clone();
    bad.key_id = UuidV4::random().unwrap();
    assert!(matches!(
        decrypt_snapshot(&bad, &meta, &snapshot.owner_github_id, &key),
        Err(Error::ContextMismatch)
    ));
    let mut bad = snapshot.clone();
    bad.kdf.memory_kib = 8;
    assert!(derive_key(
        &NormalizedPassword::new(SecretInput::new("Str0ng-Other-Phrase!".into())).unwrap(),
        &bad.kdf
    )
    .is_err());
    let mut bad = snapshot.clone();
    bad.nonce = Nonce::new([0; 24]);
    assert!(matches!(
        decrypt_snapshot(&bad, &meta, &snapshot.owner_github_id, &key),
        Err(Error::InvalidPasswordOrCiphertext)
    ));
    let wrong = DerivedKey::from_secret_bytes(Zeroizing::new([0; 32]));
    assert!(matches!(
        decrypt_snapshot(&snapshot, &meta, &snapshot.owner_github_id, &wrong),
        Err(Error::InvalidPasswordOrCiphertext)
    ));
    let mut changed = snapshot.decoded_ciphertext().unwrap();
    changed[42] ^= 1;
    let mut bad = snapshot.clone();
    bad.ciphertext = URL_SAFE_NO_PAD.encode(&changed);
    bad.ciphertext_sha256 = Sha256Digest::of(&changed);
    let mut malicious_meta = meta.clone();
    malicious_meta.ciphertext_sha256 = Some(bad.ciphertext_sha256.clone());
    assert!(matches!(
        decrypt_snapshot(&bad, &malicious_meta, &snapshot.owner_github_id, &key),
        Err(Error::InvalidPasswordOrCiphertext)
    ));
    changed.truncate(changed.len() - 1);
    bad.ciphertext = URL_SAFE_NO_PAD.encode(changed);
    assert!(bad.validate().is_err());
}

#[test]
fn new_encryption_uses_distinct_random_nonce_and_padding() {
    let vector = &vectors()[0];
    let snapshot: SnapshotUpload = serde_json::from_value(vector["snapshot"].clone()).unwrap();
    let key = test_key(vector);
    let docs =
        DocumentSet::from_json_bytes(&unhex(vector["plaintextJsonUtf8Hex"].as_str().unwrap()))
            .unwrap();
    let a = encrypt_snapshot(&docs, &context(&snapshot), &key).unwrap();
    let b = encrypt_snapshot(&docs, &context(&snapshot), &key).unwrap();
    assert_ne!(a.nonce, b.nonce);
    assert_ne!(a.ciphertext, b.ciphertext);
    assert_eq!(a.decoded_ciphertext().unwrap().len(), 65_552);
    assert!(decrypt_snapshot(&a, &metadata(&a), &a.owner_github_id, &key).is_ok());
}

#[test]
fn malformed_padding_and_oversized_json_fail_without_truncation() {
    assert_eq!(
        padded_length(MAX_PLAINTEXT_JSON_BYTES).unwrap(),
        MAX_PLAINTEXT_JSON_BYTES + PAD_BLOCK_BYTES
    );
    assert!(padded_length(MAX_PLAINTEXT_JSON_BYTES + 1).is_err());
    for declared in [0u32, u32::MAX, PAD_BLOCK_BYTES as u32] {
        let mut frame = vec![0; PAD_BLOCK_BYTES];
        frame[..4].copy_from_slice(&declared.to_be_bytes());
        assert!(decode_frame(&frame).is_err());
    }
    assert!(decode_frame(&vec![0; PAD_BLOCK_BYTES - 1]).is_err());
    assert!(!format!(
        "{:?}",
        DerivedKey::from_secret_bytes(Zeroizing::new([42; 32]))
    )
    .contains("42"));
}

#[test]
fn rfc9106_section_5_3_official_argon2id_vector() {
    // Independent published KAT for the primitive, not an alternative wire profile.
    let params = argon2::ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .output_len(32)
        .data(argon2::AssociatedData::new(&[4; 12]).unwrap())
        .build()
        .unwrap();
    let argon =
        Argon2::new_with_secret(&[3; 8], Algorithm::Argon2id, Version::V0x13, params).unwrap();
    let mut out = [0; 32];
    argon
        .hash_password_into(&[1; 32], &[2; 16], &mut out)
        .unwrap();
    assert_eq!(
        out.as_slice(),
        unhex("0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659")
    );
}

#[test]
fn local_journal_has_independent_scope_and_local_size_limit() {
    let key = DerivedKey::from_secret_bytes(Zeroizing::new([7; 32]));
    let scope = br#"["https://sync.example","123","vault","key","operation"]"#;
    let plain = vec![b'x'; MAX_LOCAL_PLAINTEXT_BYTES];
    let envelope = seal_local(&key, scope, &plain).unwrap();
    assert_eq!(
        open_local(&key, scope, &envelope).unwrap().as_slice(),
        plain.as_slice()
    );
    assert!(open_local(&key, b"different scope", &envelope).is_err());
    let mut corrupt = envelope;
    corrupt[LOCAL_ENVELOPE_MAGIC.len() + 24 + 3] ^= 1;
    assert!(open_local(&key, scope, &corrupt).is_err());
    assert!(seal_local(&key, scope, &vec![0; MAX_LOCAL_PLAINTEXT_BYTES + 1]).is_err());
    assert!(open_local(&key, scope, b"invalid").is_err());
}

#[test]
fn maximum_remote_json_roundtrips_with_bounded_pad64k_frame() {
    let vector = &vectors()[0];
    let snapshot: SnapshotUpload = serde_json::from_value(vector["snapshot"].clone()).unwrap();
    let key = test_key(vector);
    let mut docs =
        DocumentSet::from_json_bytes(&unhex(vector["plaintextJsonUtf8Hex"].as_str().unwrap()))
            .unwrap();
    docs.documents[0].value = Value::String(String::new());
    let overhead = docs.to_secret_json().unwrap().len();
    docs.documents[0].value = Value::String("x".repeat(MAX_PLAINTEXT_JSON_BYTES - overhead));
    assert_eq!(
        docs.to_secret_json().unwrap().len(),
        MAX_PLAINTEXT_JSON_BYTES
    );
    let encrypted = encrypt_snapshot(&docs, &context(&snapshot), &key).unwrap();
    assert_eq!(
        encrypted.decoded_ciphertext().unwrap().len(),
        MAX_PLAINTEXT_JSON_BYTES + PAD_BLOCK_BYTES + 16
    );
    let recovered = decrypt_snapshot(
        &encrypted,
        &metadata(&encrypted),
        &encrypted.owner_github_id,
        &key,
    )
    .unwrap();
    assert_eq!(recovered, docs);
}

#[test]
fn wrong_nonce_length_and_permuted_aad_never_authenticate() {
    let vector = &vectors()[0];
    let mut wire = vector["snapshot"].clone();
    wire["nonce"] = Value::String("AAECAwQFBgcICQoLDA0ODxAREhMUFRY".into());
    assert!(serde_json::from_value::<SnapshotUpload>(wire).is_err());

    let mut snapshot: SnapshotUpload = serde_json::from_value(vector["snapshot"].clone()).unwrap();
    let key = test_key(vector);
    let cipher = XChaCha20Poly1305::new(key.expose_bytes().into());
    let nonce = snapshot.nonce.clone();
    let mut frame = Zeroizing::new(snapshot.decoded_ciphertext().unwrap());
    cipher
        .decrypt_in_place(
            XNonce::from_slice(nonce.bytes()),
            &aad(&snapshot).unwrap(),
            &mut *frame,
        )
        .unwrap();
    let mut reordered: Value = serde_json::from_slice(&aad(&snapshot).unwrap()).unwrap();
    reordered.as_array_mut().unwrap().swap(3, 4);
    // Public deterministic fixture only; production never accepts injected nonces.
    cipher
        .encrypt_in_place(
            XNonce::from_slice(nonce.bytes()),
            &serde_json::to_vec(&reordered).unwrap(),
            &mut *frame,
        )
        .unwrap();
    snapshot.ciphertext = URL_SAFE_NO_PAD.encode(&frame);
    snapshot.ciphertext_sha256 = Sha256Digest::of(&frame);
    assert!(matches!(
        decrypt_snapshot(
            &snapshot,
            &metadata(&snapshot),
            &snapshot.owner_github_id,
            &key
        ),
        Err(Error::InvalidPasswordOrCiphertext)
    ));
}
