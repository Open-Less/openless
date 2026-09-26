use super::*;

fn document_json(value: &str) -> String {
    format!(
        r#"{{"schemaVersion":1,"exportedAt":"2026-09-23T00:00:00Z","sourceDevice":{{"id":"fixture","os":"test","arch":"test","appVersion":"0.1"}},"documents":[{{"id":"main","kind":"preferences","schemaVersion":1,"value":{value}}}],"tombstones":[]}}"#
    )
}

#[test]
fn canonical_revision_preserves_uint64_without_float_conversion() {
    let v = Revision::parse("9007199254740993").unwrap();
    assert_eq!(v.get(), 9_007_199_254_740_993);
    assert_eq!(serde_json::to_string(&v).unwrap(), "\"9007199254740993\"");
    for invalid in ["", "01", "00", "+1", "-1", "1.0", "18446744073709551616"] {
        assert!(Revision::parse(invalid).is_err());
    }
    for invalid in ["1", "1.0", "null", "true"] {
        assert!(serde_json::from_str::<Revision>(invalid).is_err());
    }
    assert!(Revision::new(u64::MAX).checked_next().is_err());
    assert!(GithubId::parse("0").is_err());
}

#[test]
fn ids_base64_hash_and_etags_are_canonical() {
    for invalid in [
        "BE406CA5-37FA-4B26-9A9A-74C1AAD48EAE",
        "be406ca5-37fa-1b26-9a9a-74c1aad48eae",
        "be406ca537fa4b269a9a74c1aad48eae",
    ] {
        assert!(UuidV4::parse(invalid).is_err());
    }
    let id = UuidV4::random().unwrap();
    assert!(UuidV4::parse(id.as_str()).is_ok());
    assert!(Salt::parse("AAECAwQFBgcICQoLDA0ODw").is_ok());
    for invalid in [
        "AAECAwQFBgcICQoLDA0ODw==",
        "AAECAwQFBgcICQoLDA0ODx",
        "AAECAwQFBgcICQoLDA0OD+",
    ] {
        assert!(Salt::parse(invalid).is_err());
    }
    assert!(Sha256Digest::parse(&"AB".repeat(32)).is_err());
    for invalid in ["*", "W/\"etag\"", "etag", "\"\"", "\"a\"b\"", "\"a\nb\""] {
        assert!(StrongEtag::parse(invalid).is_err());
    }
    assert_eq!(
        StrongEtag::parse("\"opaque,=!\\\"").unwrap().as_str(),
        "\"opaque,=!\\\""
    );
}

#[test]
fn private_json_rejects_duplicates_depth_and_bad_unicode() {
    assert!(DocumentSet::from_json_bytes(document_json(r#"{"a":1,"a":2}"#).as_bytes()).is_err());
    let root_duplicate = document_json("{}").replacen(
        "\"schemaVersion\":1",
        "\"schemaVersion\":1,\"schemaVersion\":1",
        1,
    );
    assert!(DocumentSet::from_json_bytes(root_duplicate.as_bytes()).is_err());
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(DocumentSet::from_json_bytes(document_json(&deep).as_bytes()).is_err());
    assert!(DocumentSet::from_json_bytes(document_json(r#"{"bad":1e400}"#).as_bytes()).is_err());
    assert!(DocumentSet::from_json_bytes(&[0xff, 0xfe]).is_err());
    assert!(
        DocumentSet::from_json_bytes(format!("{} null", document_json("{}")).as_bytes()).is_err()
    );
}

#[test]
fn document_identity_and_schema_are_validated_without_dropping_extensions() {
    let input = document_json(r#"{"futureSetting":{"secret":"fixture"},"value":3}"#);
    let mut docs = DocumentSet::from_json_bytes(input.as_bytes()).unwrap();
    let bytes = docs.to_secret_json().unwrap();
    let restored = DocumentSet::from_json_bytes(&bytes).unwrap();
    assert_eq!(
        restored.documents[0].value["futureSetting"]["secret"],
        "fixture"
    );
    assert!(!format!("{docs:?}").contains("fixture"));
    docs.documents.push(docs.documents[0].clone());
    assert!(docs.validate().is_err());
    docs.documents.pop();
    docs.tombstones.push(Tombstone {
        id: "main".into(),
        kind: DocumentKind::Preferences,
        deleted_at: "2026-09-23T00:00:00Z".into(),
        base_revision: Revision::new(0),
    });
    assert!(docs.validate().is_err());
    assert!(matches!(
        DocumentSet::from_json_bytes(
            input
                .replace("\"preferences\"", "\"future_kind\"")
                .as_bytes()
        ),
        Err(Error::UnsupportedDocumentVersion)
    ));
    docs.tombstones.clear();
    docs.documents[0].schema_version = 2;
    assert!(matches!(
        docs.validate(),
        Err(Error::UnsupportedDocumentVersion)
    ));
}

#[test]
fn metadata_requires_null_fields_and_rejects_inconsistent_states() {
    let valid = r#"{"protocolVersion":1,"state":"empty","ownerGithubId":"12345","revision":"0","vaultId":null,"keyId":null,"updatedAt":null,"payloadSchemaVersion":null,"ciphertextBytes":0,"ciphertextSha256":null,"lastOperationId":null}"#;
    assert!(parse_wire::<VaultMetadata>(valid.as_bytes(), CONTROL_BODY_LIMIT).is_ok());
    assert!(parse_wire::<VaultMetadata>(
        valid.replace("\"keyId\":null,", "").as_bytes(),
        CONTROL_BODY_LIMIT
    )
    .is_err());
    assert!(parse_wire::<VaultMetadata>(
        valid
            .replace("\"revision\":\"0\"", "\"revision\":\"1\"")
            .as_bytes(),
        CONTROL_BODY_LIMIT
    )
    .is_err());
    assert!(parse_wire::<VaultMetadata>(
        valid.replace("\"empty\"", "\"active\"").as_bytes(),
        CONTROL_BODY_LIMIT
    )
    .is_err());
}

#[test]
fn schema_and_errors_never_echo_attacker_content() {
    let marker = "PASSWORD_AND_TOKEN_MUST_NEVER_APPEAR";
    let input = document_json("{}").replace(
        "\"schemaVersion\":1",
        &format!("\"schemaVersion\":\"{marker}\""),
    );
    let err = DocumentSet::from_json_bytes(input.as_bytes()).unwrap_err();
    assert!(!format!("{err:?} {err}").contains(marker));
    assert!(!format!("{:?}", SecretInput::new(marker.into())).contains(marker));
    assert!(!format!("{:?}", SecretToken::new(marker.into()).unwrap()).contains(marker));
}

#[test]
fn control_response_limit_is_enforced_before_json_parsing() {
    let bytes = vec![b' '; CONTROL_BODY_LIMIT + 1];
    assert!(matches!(
        parse_wire::<VaultMetadata>(&bytes, CONTROL_BODY_LIMIT),
        Err(Error::PayloadTooLarge)
    ));
}

#[test]
fn export_checks_depth_and_limit_before_allocating_json_output() {
    let mut docs = DocumentSet::from_json_bytes(document_json("null").as_bytes()).unwrap();
    docs.documents[0].value = Value::String("x".repeat(MAX_PLAINTEXT_JSON_BYTES));
    assert!(matches!(docs.to_secret_json(), Err(Error::PayloadTooLarge)));
    let mut value = Value::Null;
    for _ in 0..65 {
        value = Value::Array(vec![value]);
    }
    docs.documents[0].value = value;
    assert!(docs.to_secret_json().is_err());
}

#[test]
fn optional_current_revision_must_not_be_explicit_null() {
    let wire = r#"{"error":{"code":"revision_conflict","message":"fixed","requestId":"e1d8c32e-d209-4e56-8735-bc66b8684c71","currentRevision":null}}"#;
    assert!(parse_wire::<ErrorEnvelope>(wire.as_bytes(), CONTROL_BODY_LIMIT).is_err());
    assert!(parse_wire::<ErrorEnvelope>(
        wire.replace(",\"currentRevision\":null", "").as_bytes(),
        CONTROL_BODY_LIMIT
    )
    .is_ok());
}
