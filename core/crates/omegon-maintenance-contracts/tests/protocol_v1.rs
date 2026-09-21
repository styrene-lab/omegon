use std::{
    fs::OpenOptions,
    process::Command,
    sync::{Arc, Barrier},
    thread,
};

use omegon_maintenance_contracts::{
    ArtifactIdentityV1, AuditCheckpointV1, AuditFrontierV1, AuditReceiptV1, AuditRecordV1,
    AuthorityKey, CleanupCapability, CommandSemanticsV1, ContributionKind, ContributionSelector,
    DenyRecordV1, DenyState, DenyStateV1, DetachObservation, ErrorV1, FenceState, FenceV1,
    FileIdentityV1, InstallationStateV1, LifecycleBoundary, ListScope, LockMode,
    MaintenanceResultV1, MaintenanceStateV1, MutationResultV1, MutationState, OwnershipRecordV1,
    PackageManifestV1, PathIdentityV1, PostStateV1, ProtocolLock, ReconciliationDecision, Record,
    RecordObservation, ResultStatus, SCHEMA_VERSION, SessionDenyRecordV1, SessionDenyState,
    TransactionState, TransactionStepKind, TransactionStepState, TransactionStepV1, TransactionV1,
    append_bytes_at, canonical_digest, canonical_json, command_fingerprint,
    contribution_domain_key, create_record_no_replace_at, derive_key, entry_key,
    normalize_workspace_path, open_secure_dir_at, open_secure_root, parse_record, path_identity,
    read_bytes_at, read_record_at, reconcile_detach, reconcile_record, record_identity_at,
    replace_record_at, resolve_list_scope, scope_key, session_key, validate_child_name,
    workspace_key,
};
use sha2::{Digest, Sha256};

const ZERO_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn installation() -> InstallationStateV1 {
    let home = PathIdentityV1::unix(b"/tmp/omegon", 7, 11).unwrap();
    InstallationStateV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "installation_state".into(),
        record_id: derive_key("installation", &[b"11111111-1111-1111-1111-111111111111"]),
        installation_uuid: "11111111-1111-1111-1111-111111111111".into(),
        home,
        next_audit_sequence: 1,
    }
}

#[test]
fn ownership_constructor_and_refresh_preserve_identity() {
    let workspace = workspace_key("unix", b"/tmp/omegon-workspace");
    let writer = ArtifactIdentityV1 {
        version: "1.0.0".into(),
        commit: "abcdef0".into(),
        target: "x86_64-unknown-linux-gnu".into(),
        digest: AuthorityKey::from_bytes([7; 32]),
    };
    let mut record = OwnershipRecordV1::new(
        "runtime-test".into(),
        "generation-test".into(),
        workspace,
        "linux:00000000-0000-0000-0000-000000000001".into(),
        42,
        None,
        "linux:123".into(),
        LifecycleBoundary::OwnedProcessTree,
        CleanupCapability::BestEffort,
        writer,
        "2026-08-19T00:00:00Z".into(),
        100,
    )
    .unwrap();
    let identity = record.record_id;
    record
        .refresh_heartbeat("2026-08-19T00:01:00Z".into(), 200)
        .unwrap();
    assert_eq!(record.record_id, identity);
    assert_eq!(record.expires_after_seconds, 300);
    assert_eq!(record.heartbeat_monotonic_ticks, 200);
}

#[test]
fn canonical_fixture_is_stable_and_round_trips() {
    let record = installation();
    let encoded = canonical_json(&record).unwrap();
    assert_eq!(
        encoded,
        include_bytes!("fixtures/installation-state-v1.json")
    );
    assert_eq!(
        parse_record::<InstallationStateV1>(&encoded).unwrap(),
        record
    );
}

#[test]
fn every_authority_record_has_a_canonical_fixture() {
    assert_fixture::<DenyRecordV1>(include_bytes!("fixtures/deny-v1.json"));
    assert_fixture::<DenyStateV1>(include_bytes!("fixtures/deny-state-v1.json"));
    assert_fixture::<SessionDenyRecordV1>(include_bytes!("fixtures/session-deny-v1.json"));
    assert_fixture::<OwnershipRecordV1>(include_bytes!("fixtures/ownership-v1.json"));
    assert_fixture::<TransactionV1>(include_bytes!("fixtures/transaction-v1.json"));
    assert_fixture::<FenceV1>(include_bytes!("fixtures/fence-v1.json"));
    assert_fixture::<AuditRecordV1>(include_bytes!("fixtures/audit-v1.json"));
    assert_fixture::<AuditCheckpointV1>(include_bytes!("fixtures/audit-checkpoint-v1.json"));
    assert_fixture::<AuditFrontierV1>(include_bytes!("fixtures/audit-frontier-v1.json"));
    assert_fixture::<AuditReceiptV1>(include_bytes!("fixtures/audit-receipt-v1.json"));
    assert_fixture::<PackageManifestV1>(include_bytes!("fixtures/package-manifest-v1.json"));
}

#[test]
fn package_manifest_distribution_composition_is_additive_and_typed() {
    let legacy = include_bytes!("fixtures/package-manifest-v1.json");
    assert!(parse_record::<PackageManifestV1>(legacy).is_ok());
    let encode = |value: &serde_json::Value| {
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.push(b'\n');
        bytes
    };

    let mut value: serde_json::Value = serde_json::from_slice(legacy).unwrap();
    value["host_profile"] = serde_json::json!("full-product");
    value["composition_class"] = serde_json::json!("full-product");
    value["core_components"] = serde_json::json!([{
        "component_id": "core:codescan",
        "wire_manifest_id": "omegon-codescan"
    }]);
    value["sdk_extension_posture"] = serde_json::json!("operator-managed");
    assert!(parse_record::<PackageManifestV1>(&encode(&value)).is_err());
    value["members"].as_array_mut().unwrap().extend([
        serde_json::json!({
            "digest": ZERO_KEY,
            "mode": 420,
            "path": "share/omegon/extensions/omegon-codescan/manifest.toml",
            "size": 1
        }),
        serde_json::json!({
            "digest": ZERO_KEY,
            "mode": 493,
            "path": "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan",
            "size": 1
        }),
        serde_json::json!({
            "digest": ZERO_KEY,
            "mode": 420,
            "path": "share/omegon/components/core-codescan.lock.json",
            "size": 1
        }),
    ]);
    value["product_component_locks"] = serde_json::json!([{
        "schema_version": 1,
        "component_id": "core:codescan",
        "wire_manifest_id": "omegon-codescan",
        "manifest_path": "share/omegon/extensions/omegon-codescan/manifest.toml",
        "manifest_digest": ZERO_KEY,
        "executable_path": "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan",
        "executable_digest": ZERO_KEY,
        "target": "x86_64-unknown-linux-gnu",
        "protocol_minimum": 1,
        "protocol_maximum": 1,
        "protocol_version": 1,
        "fallback": "typed_unavailable",
        "signing_identity": {
            "issuer": "https://token.actions.githubusercontent.com",
            "workflow_identity": "https://github.com/styrene-lab/omegon/.github/workflows/release.yml@refs/tags/v0.29.0-dev",
            "verification": "required"
        }
    }]);
    let parsed = parse_record::<PackageManifestV1>(&encode(&value));
    assert!(
        parsed.is_ok(),
        "typed package manifest rejected: {parsed:?}"
    );

    value["core_components"][0]["component_id"] = serde_json::json!("sdk:self-promoted");
    assert!(parse_record::<PackageManifestV1>(&encode(&value)).is_err());
}

#[test]
fn full_product_package_requires_independent_codescan_evidence() {
    let fixture = include_bytes!("fixtures/package-manifest-v1.json");
    let mut value: serde_json::Value = serde_json::from_slice(fixture).unwrap();
    value["host_profile"] = serde_json::json!("full-product");
    value["composition_class"] = serde_json::json!("full-product");
    value["core_components"] = serde_json::json!([{
        "component_id": "core:codescan",
        "wire_manifest_id": "omegon-codescan"
    }]);
    value["sdk_extension_posture"] = serde_json::json!("operator-managed");
    value["members"].as_array_mut().unwrap().extend([
        serde_json::json!({"digest": ZERO_KEY, "mode": 420, "path": "share/omegon/extensions/omegon-codescan/manifest.toml", "size": 1}),
        serde_json::json!({"digest": ZERO_KEY, "mode": 493, "path": "share/omegon/extensions/omegon-codescan/target/release/omegon-codescan", "size": 1}),
    ]);
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    assert!(parse_record::<PackageManifestV1>(&bytes).is_err());
}

#[test]
fn result_wire_fixture_is_bounded_and_stable() {
    for bytes in [
        include_bytes!("fixtures/result-v1.json").as_slice(),
        include_bytes!("fixtures/result-degraded-v1.json").as_slice(),
        include_bytes!("fixtures/result-failure-v1.json").as_slice(),
    ] {
        let result: MaintenanceResultV1 = serde_json::from_slice(bytes).unwrap();
        result.validate().unwrap();
        assert_eq!(canonical_json(&result).unwrap(), bytes);
    }
}

#[test]
fn result_validation_rejects_unstable_codes_and_unsafe_truncation() {
    let mut result: MaintenanceResultV1 =
        serde_json::from_slice(include_bytes!("fixtures/result-v1.json")).unwrap();
    result.errors.push(ErrorV1 {
        code: "unknown_family".into(),
        phase: "admission".into(),
        retry_safe: true,
        message: "refused".into(),
    });
    assert!(result.validate().is_err());

    result.errors[0].code = "root_unsafe".into();
    assert!(result.validate().is_err(), "success cannot contain errors");

    result.errors.clear();
    result.mutations.push(MutationResultV1 {
        domain_key: ZERO_KEY.parse().unwrap(),
        kind: "deny".into(),
        state: MutationState::Unknown,
        retry_safe: false,
    });
    assert!(
        result.validate().is_err(),
        "success cannot contain an unsettled mutation"
    );
    result.mutations.clear();

    result.status = ResultStatus::Degraded;
    result.command = "contribution.list".into();
    result.truncated = true;
    result.next_cursor = Some("next-page".into());
    result.mutations.push(MutationResultV1 {
        domain_key: ZERO_KEY.parse().unwrap(),
        kind: "deny".into(),
        state: MutationState::Applied,
        retry_safe: false,
    });
    assert!(
        result.validate().is_err(),
        "applied mutation cannot be truncated"
    );

    result.mutations.clear();
    result.errors.clear();
    assert!(
        result.validate().is_ok(),
        "paginated list diagnostics may truncate with a continuation cursor"
    );
    result.command = "audit.inspect".into();
    assert!(
        result.validate().is_ok(),
        "audit inspection may truncate with a continuation cursor"
    );

    result.truncated = false;
    result.next_cursor = None;
    result.composition.excluded_inputs =
        vec!["x".repeat(omegon_maintenance_contracts::MAX_RESULT_BYTES)];
    assert!(
        result.validate().is_err(),
        "result envelope must be bounded"
    );
}

#[test]
fn command_fingerprint_accepts_only_typed_semantics() {
    let mut semantics: CommandSemanticsV1 =
        serde_json::from_slice(include_bytes!("fixtures/command-semantics-v1.json")).unwrap();
    assert_eq!(
        command_fingerprint(&semantics).unwrap().to_string(),
        "91d4b6aca1f2f76e46fb28f32563fc47e110ea01ddf8e51680140cbc25193a49"
    );
    semantics
        .semantic_options
        .insert("request_id".into(), serde_json::json!("forbidden"));
    assert!(command_fingerprint(&semantics).is_err());
}

fn assert_fixture<T>(bytes: &[u8])
where
    T: omegon_maintenance_contracts::Record + for<'de> serde::Deserialize<'de> + serde::Serialize,
{
    let parsed: T = parse_record(bytes).unwrap();
    assert_eq!(canonical_json(&parsed).unwrap(), bytes);
}

#[test]
fn canonical_key_vector_is_stable() {
    let actual = derive_key("path", &[b"unix", b"/tmp/omegon"]);
    assert_eq!(
        actual.to_string(),
        "28b131518772d1d41891ed1d23b793fbc788c897d9aed418b8fb2515937229f7"
    );
    assert_eq!(
        derive_key("installation", &[b"11111111-1111-1111-1111-111111111111"]).to_string(),
        "a025a02cc5ba0d43c08b35ede56fa60321bea3798b577c359d6e9532a3ec707b"
    );
    assert_eq!(
        omegon_maintenance_contracts::session_key(
            "2026-08-17T00-00-00_00000000",
            ZERO_KEY.parse().unwrap()
        )
        .to_string(),
        "145442d96433837fcbd9304d1606736e040c50c3c8323b521f8c853b43e33216"
    );
    assert_eq!(
        derive_key(
            "session-deny",
            &[
                &hex_key("145442d96433837fcbd9304d1606736e040c50c3c8323b521f8c853b43e33216"),
                b"00000000-0000-0000-0000-000000000001"
            ]
        )
        .to_string(),
        "1c2fa0b9f27c6b2f879e32f0c0c47b5e2509f812320464b4b75471553812ed77"
    );
}

#[test]
fn every_authority_key_and_digest_has_an_independent_vector() {
    let zero: AuthorityKey = ZERO_KEY.parse().unwrap();
    let vectors = [
        (
            omegon_maintenance_contracts::workspace_key("unix", b"/tmp/omegon"),
            "2086da2783b16dbb6012a36164b5bdb5ddb168675373cd4f198aa44e6d227782",
        ),
        (
            omegon_maintenance_contracts::scope_key("plugin", "user", zero),
            "9e63c5d56ec80266bdc4a4f76c580149dfac9e3535d30daab1ea0cc93edaf1db",
        ),
        (
            omegon_maintenance_contracts::entry_key("plugin", zero, b"formatter"),
            "298038f9f86ae24a5c4b7e475255c3d660280c97dd4872ee419c6a692123a912",
        ),
        (
            omegon_maintenance_contracts::resource_domain_key(zero),
            "1051c37f5c933d1e5cab029075c9e9e5ae33ef61a096646d1da4006fad3a8ae3",
        ),
        (
            omegon_maintenance_contracts::contribution_domain_key(zero),
            "1dbe2a0fd6873fe7fbfbcafafd493c505ea7a55a659b4a9aedbb62c34d519d30",
        ),
        (
            omegon_maintenance_contracts::session_domain_key(zero),
            "c03f443564f61eb19dd89573a72d880d33afa46fa194ade77f44096e4383602a",
        ),
        (
            omegon_maintenance_contracts::canonical_digest(&installation()).unwrap(),
            "9665a68bbdae3ecf1ed47d22f23626f155a24abc8d6209ef062e6f52ab02a437",
        ),
    ];
    for (actual, expected) in vectors {
        assert_eq!(actual.to_string(), expected);
    }
}

fn hex_key(value: &str) -> [u8; 32] {
    *value.parse::<AuthorityKey>().unwrap().as_bytes()
}

#[test]
fn parser_rejects_corrupt_authority_records() {
    let valid = canonical_json(&installation()).unwrap();

    let duplicate =
        String::from_utf8(valid.clone())
            .unwrap()
            .replacen("{", "{\"schema_version\":1,", 1);
    assert!(matches!(
        parse_record::<InstallationStateV1>(duplicate.as_bytes()),
        Err(omegon_maintenance_contracts::ContractError::DuplicateKey(_))
    ));

    let unknown =
        String::from_utf8(valid.clone())
            .unwrap()
            .replacen("{", "{\"unexpected\":true,", 1);
    assert!(parse_record::<InstallationStateV1>(unknown.as_bytes()).is_err());

    let float = String::from_utf8(valid.clone())
        .unwrap()
        .replace("\"next_audit_sequence\":1", "\"next_audit_sequence\":1.5");
    assert!(parse_record::<InstallationStateV1>(float.as_bytes()).is_err());

    let future = String::from_utf8(valid)
        .unwrap()
        .replace("\"schema_version\":1", "\"schema_version\":2");
    assert!(parse_record::<InstallationStateV1>(future.as_bytes()).is_err());

    assert!(parse_record::<InstallationStateV1>(valid_without_lf()).is_err());
    let mut extra_lf = canonical_json(&installation()).unwrap();
    extra_lf.push(b'\n');
    assert!(parse_record::<InstallationStateV1>(&extra_lf).is_err());

    let whitespace = String::from_utf8(canonical_json(&installation()).unwrap())
        .unwrap()
        .replacen(":", ": ", 1);
    assert!(parse_record::<InstallationStateV1>(whitespace.as_bytes()).is_err());

    let nested_duplicate = String::from_utf8(canonical_json(&installation()).unwrap())
        .unwrap()
        .replace("\"device\":7", "\"device\":7,\"device\":7");
    assert!(parse_record::<InstallationStateV1>(nested_duplicate.as_bytes()).is_err());

    let alternate_escape = String::from_utf8(canonical_json(&installation()).unwrap())
        .unwrap()
        .replace("11111111-1111", "11111111\\u002d1111");
    assert!(parse_record::<InstallationStateV1>(alternate_escape.as_bytes()).is_err());

    let signed_zero = include_str!("fixtures/ownership-v1.json")
        .replace("\"process_group\":42", "\"process_group\":-0");
    assert!(parse_record::<OwnershipRecordV1>(signed_zero.as_bytes()).is_err());

    let oversized = vec![b'x'; omegon_maintenance_contracts::MAX_RECORD_BYTES + 1];
    assert!(parse_record::<InstallationStateV1>(&oversized).is_err());
}

#[test]
fn parser_rejects_semantically_contradictory_records() {
    let bad_session = include_str!("fixtures/session-deny-v1.json").replace(
        "\"session_key\":\"145442d96433837fcbd9304d1606736e040c50c3c8323b521f8c853b43e33216\"",
        &format!("\"session_key\":\"{ZERO_KEY}\""),
    );
    assert!(parse_record::<SessionDenyRecordV1>(bad_session.as_bytes()).is_err());

    let existing = "{\"device\":1,\"inode\":1,\"modified_ns\":1,\"size\":1}";
    let contradictory_step = include_str!("fixtures/transaction-v1.json").replace(
        "\"expected_existing\":null",
        &format!("\"expected_existing\":{existing}"),
    );
    assert!(parse_record::<TransactionV1>(contradictory_step.as_bytes()).is_err());

    let contradictory_state = include_str!("fixtures/transaction-v1.json").replace(
        "\"state\":\"prepared\",\"steps\"",
        "\"state\":\"settled\",\"steps\"",
    );
    assert!(parse_record::<TransactionV1>(contradictory_state.as_bytes()).is_err());

    let bad_timestamp = include_str!("fixtures/deny-v1.json")
        .replace("2026-08-17T00:00:00Z", "2026-02-31T00:00:00Z");
    assert!(parse_record::<DenyRecordV1>(bad_timestamp.as_bytes()).is_err());

    let bad_request = include_str!("fixtures/audit-v1.json")
        .replace("00000000-0000-0000-0000-000000000001", "NOT-A-UUID");
    assert!(parse_record::<AuditRecordV1>(bad_request.as_bytes()).is_err());

    let bad_frontier = include_str!("fixtures/audit-frontier-v1.json").replace(
        "\"previous_segment_start\":1",
        "\"previous_segment_start\":2",
    );
    assert!(parse_record::<AuditFrontierV1>(bad_frontier.as_bytes()).is_err());

    let bad_receipt =
        include_str!("fixtures/audit-receipt-v1.json").replace("\"sequence\":1", "\"sequence\":0");
    assert!(parse_record::<AuditReceiptV1>(bad_receipt.as_bytes()).is_err());

    let bad_boot = include_str!("fixtures/ownership-v1.json").replace(
        "linux:00000000-0000-0000-0000-000000000001",
        "different-boot",
    );
    assert!(parse_record::<OwnershipRecordV1>(bad_boot.as_bytes()).is_err());

    let bad_process =
        include_str!("fixtures/ownership-v1.json").replace("linux:42", "different-token");
    assert!(parse_record::<OwnershipRecordV1>(bad_process.as_bytes()).is_err());
}

#[test]
fn transaction_frontier_and_evidence_matrix_is_strict() {
    let base: TransactionV1 = parse_record(include_bytes!("fixtures/transaction-v1.json")).unwrap();

    let mut dispatched = base.clone();
    dispatched.state = TransactionState::StepDispatched;
    dispatched.steps[0].state = TransactionStepState::Dispatched;
    dispatched.validate().unwrap();

    let mut settled = base.clone();
    settled.state = TransactionState::Settled;
    settled.audit_sequence = Some(1);
    settled.steps[0].state = TransactionStepState::Settled;
    settled.steps[0].observed = Some(PostStateV1 {
        source_present: false,
        destination: Some(FileIdentityV1 {
            device: 7,
            inode: 12,
            size: 1,
            modified_ns: 1,
        }),
        destination_content_digest: settled.steps[0].intended_content_digest,
    });
    settled.validate().unwrap();

    let mut contradictory_create = settled.clone();
    contradictory_create.steps[0]
        .observed
        .as_mut()
        .unwrap()
        .source_present = true;
    assert!(contradictory_create.validate().is_err());

    let mut targets_settled = settled.clone();
    targets_settled.state = TransactionState::TargetsSettled;
    targets_settled.audit_sequence = None;
    targets_settled.validate().unwrap();

    let mut step_settled = settled.clone();
    step_settled.state = TransactionState::StepSettled;
    step_settled.audit_sequence = None;
    step_settled.steps.push(base.steps[0].clone());
    step_settled.validate().unwrap();

    let mut impossible_frontier = base.clone();
    impossible_frontier.state = TransactionState::StepDispatched;
    impossible_frontier.steps.push(dispatched.steps[0].clone());
    assert!(impossible_frontier.validate().is_err());

    let mut prepared_with_evidence = base.clone();
    prepared_with_evidence.steps[0].observed = settled.steps[0].observed.clone();
    assert!(prepared_with_evidence.validate().is_err());

    let mut settled_without_evidence = settled.clone();
    settled_without_evidence.steps[0].observed = None;
    assert!(settled_without_evidence.validate().is_err());

    let mut create_over_existing = base.clone();
    create_over_existing.steps[0].expected_absence = false;
    create_over_existing.steps[0].expected_existing = Some(FileIdentityV1 {
        device: 7,
        inode: 12,
        size: 1,
        modified_ns: 1,
    });
    assert!(create_over_existing.validate().is_err());

    let mut replace_over_absence = base.clone();
    replace_over_absence.steps[0].kind = TransactionStepKind::DenyStateReplace;
    assert!(replace_over_absence.validate().is_err());

    let mut detach_with_content = create_over_existing;
    detach_with_content.steps[0].kind = TransactionStepKind::QuarantineDetach;
    assert!(detach_with_content.validate().is_err());

    let mut audited_active = base.clone();
    audited_active.audit_sequence = Some(1);
    assert!(audited_active.validate().is_err());

    let mut time_reversed = base;
    time_reversed.created_at = "2026-08-18T00:00:00Z".into();
    assert!(time_reversed.validate().is_err());
}

#[test]
fn quarantine_steps_bind_rename_destination_and_distinguish_symlink_unlink() {
    let base: TransactionV1 = parse_record(include_bytes!("fixtures/transaction-v1.json")).unwrap();
    let identity = FileIdentityV1 {
        device: 7,
        inode: 12,
        size: 1,
        modified_ns: 1,
    };

    let mut rename = base.clone();
    rename.steps[0].kind = TransactionStepKind::QuarantineDetach;
    rename.steps[0].expected_absence = false;
    rename.steps[0].expected_existing = Some(identity.clone());
    rename.steps[0].intended_content_digest = None;
    rename.steps[0].destination_parent = Some(rename.steps[0].parent.clone());
    let (destination_bytes, destination_digest) =
        TransactionStepV1::encode_basename(b"destination").unwrap();
    rename.steps[0].destination_basename_bytes = Some(destination_bytes.clone());
    rename.steps[0].destination_basename_digest = Some(destination_digest);
    rename.steps[0].state = TransactionStepState::Settled;
    rename.steps[0].observed = Some(PostStateV1 {
        source_present: false,
        destination: Some(identity.clone()),
        destination_content_digest: None,
    });
    rename.state = TransactionState::Settled;
    rename.audit_sequence = Some(1);
    rename.validate().unwrap();

    let mut mismatched_destination = rename.clone();
    mismatched_destination.steps[0]
        .observed
        .as_mut()
        .unwrap()
        .destination = Some(FileIdentityV1 {
        inode: 13,
        ..identity.clone()
    });
    assert!(mismatched_destination.validate().is_err());

    let mut unbound_rename = rename.clone();
    unbound_rename.steps[0].destination_parent = None;
    unbound_rename.steps[0].destination_basename_bytes = None;
    unbound_rename.steps[0].destination_basename_digest = None;
    assert!(unbound_rename.validate().is_err());

    let mut unlink = base;
    unlink.steps[0].kind = TransactionStepKind::QuarantineSymlinkUnlink;
    unlink.steps[0].expected_absence = false;
    unlink.steps[0].expected_existing = Some(identity);
    unlink.steps[0].intended_content_digest = None;
    unlink.steps[0].state = TransactionStepState::Settled;
    unlink.steps[0].observed = Some(PostStateV1 {
        source_present: false,
        destination: None,
        destination_content_digest: None,
    });
    unlink.state = TransactionState::Settled;
    unlink.audit_sequence = Some(1);
    unlink.validate().unwrap();

    let mut unlink_with_destination = unlink;
    unlink_with_destination.steps[0].destination_parent = Some(rename.steps[0].parent.clone());
    unlink_with_destination.steps[0].destination_basename_bytes = Some(destination_bytes);
    unlink_with_destination.steps[0].destination_basename_digest = Some(destination_digest);
    assert!(unlink_with_destination.validate().is_err());
}

fn valid_without_lf() -> &'static [u8] {
    include_bytes!("fixtures/installation-state-v1.json")
        .strip_suffix(b"\n")
        .unwrap()
}

#[test]
fn parser_accepts_reordered_object_keys_only() {
    let reordered = concat!(
        "{\"schema_version\":1,\"record_kind\":\"deny_state\",",
        "\"record_id\":\"c2a87c0027634788428939b4d5170a05bc4e191ca1760a47397da5f4b8d1f521\",",
        "\"scope_key\":\"0000000000000000000000000000000000000000000000000000000000000000\",",
        "\"generation\":0,\"entries\":{}}\n"
    );
    assert!(parse_record::<DenyStateV1>(reordered.as_bytes()).is_ok());
}

#[test]
fn every_fixture_rejects_a_forged_record_id() {
    assert_bad_record_id::<InstallationStateV1>(include_bytes!(
        "fixtures/installation-state-v1.json"
    ));
    assert_bad_record_id::<DenyRecordV1>(include_bytes!("fixtures/deny-v1.json"));
    assert_bad_record_id::<DenyStateV1>(include_bytes!("fixtures/deny-state-v1.json"));
    assert_bad_record_id::<SessionDenyRecordV1>(include_bytes!("fixtures/session-deny-v1.json"));
    assert_bad_record_id::<OwnershipRecordV1>(include_bytes!("fixtures/ownership-v1.json"));
    assert_bad_record_id::<TransactionV1>(include_bytes!("fixtures/transaction-v1.json"));
    assert_bad_record_id::<FenceV1>(include_bytes!("fixtures/fence-v1.json"));
    assert_bad_record_id::<AuditRecordV1>(include_bytes!("fixtures/audit-v1.json"));
    assert_bad_record_id::<AuditCheckpointV1>(include_bytes!("fixtures/audit-checkpoint-v1.json"));
    assert_bad_record_id::<AuditFrontierV1>(include_bytes!("fixtures/audit-frontier-v1.json"));
    assert_bad_record_id::<AuditReceiptV1>(include_bytes!("fixtures/audit-receipt-v1.json"));
    assert_bad_record_id::<PackageManifestV1>(include_bytes!("fixtures/package-manifest-v1.json"));
}

#[test]
fn corruption_fixture_inventory_is_cross_consumer_data() {
    #[derive(serde::Deserialize)]
    struct CorruptionCase {
        fixture: String,
        field: String,
        replacement: String,
        expected_error: String,
    }
    let cases: Vec<CorruptionCase> =
        serde_json::from_slice(include_bytes!("fixtures/corruption-cases-v1.json")).unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        assert!(case.fixture.ends_with("-v1.json"));
        assert_eq!(case.field, "record_id");
        assert_eq!(case.replacement, ZERO_KEY);
        assert_eq!(case.expected_error, "record_invalid_id");
    }
}

fn assert_bad_record_id<T>(fixture: &[u8])
where
    T: omegon_maintenance_contracts::Record + for<'de> serde::Deserialize<'de>,
{
    let mut corrupt = fixture.to_vec();
    let marker = b"\"record_id\":\"";
    let offset = corrupt
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    corrupt[offset..offset + 64].fill(b'0');
    if fixture[offset..offset + 64]
        .iter()
        .all(|byte| *byte == b'0')
    {
        corrupt[offset] = b'1';
    }
    assert!(parse_record::<T>(&corrupt).is_err());
}

#[test]
fn authority_keys_require_lowercase_fixed_width_hex() {
    assert!(ZERO_KEY.parse::<AuthorityKey>().is_ok());
    assert!(
        "28B131518772D1D41891ED1D23B793FBC788C897D9AED418B8FB2515937229F7"
            .parse::<AuthorityKey>()
            .is_err()
    );
    assert!("00".parse::<AuthorityKey>().is_err());
    assert!(
        format!("{}g", &ZERO_KEY[..63])
            .parse::<AuthorityKey>()
            .is_err()
    );
}

#[test]
fn selectors_reject_traversal_and_unknown_kinds() {
    assert!("plugin:formatter".parse::<ContributionSelector>().is_ok());
    assert!(
        format!("entry:sha256:{ZERO_KEY}")
            .parse::<ContributionSelector>()
            .is_ok()
    );

    for invalid in [
        "plugin:../escape",
        "plugin:nested/path",
        "plugin:",
        "unknown:item",
        "plugin:\0item",
    ] {
        assert!(
            invalid.parse::<ContributionSelector>().is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn byte_paths_normalize_without_following_links() {
    assert_eq!(
        normalize_workspace_path(b"/tmp/./project/../workspace").unwrap(),
        b"/tmp/workspace"
    );
    assert_eq!(
        normalize_workspace_path(b"/tmp/non-utf8-\xff").unwrap(),
        b"/tmp/non-utf8-\xff"
    );
    assert!(normalize_workspace_path(b"/../../escape").is_err());
    for child in [b"..".as_slice(), b"nested/path", b"C:prefix", b"\0name"] {
        assert!(validate_child_name(child).is_err());
    }
    assert!(validate_child_name(b"backslash\\is-data-on-unix").is_ok());
    assert!(PathIdentityV1::unix(b"/tmp/../escape", 1, 1).is_err());
}

#[test]
fn contribution_list_scope_matrix_is_explicit() {
    assert_eq!(resolve_list_scope(None, false).unwrap(), ListScope::User);
    assert_eq!(
        resolve_list_scope(Some("user"), false).unwrap(),
        ListScope::User
    );
    assert_eq!(
        resolve_list_scope(None, true).unwrap(),
        ListScope::UserAndProject
    );
    assert_eq!(
        resolve_list_scope(Some("project"), true).unwrap(),
        ListScope::Project
    );
    assert!(resolve_list_scope(Some("user"), true).is_err());
    assert!(resolve_list_scope(Some("project"), false).is_err());
}

#[test]
fn result_status_has_stable_exit_codes() {
    assert_eq!(ResultStatus::Success.exit_code(), 0);
    assert_eq!(ResultStatus::Failure.exit_code(), 1);
    assert_eq!(ResultStatus::Degraded.exit_code(), 2);
}

#[test]
fn detach_reconciliation_is_conservative_at_every_crash_point() {
    let cases = [
        (
            DetachObservation {
                dispatched: false,
                source_matches: true,
                destination_matches: false,
                conflicting_state: false,
                observable: true,
            },
            ReconciliationDecision::AbortAndClearFence,
        ),
        (
            DetachObservation {
                dispatched: true,
                source_matches: false,
                destination_matches: true,
                conflicting_state: false,
                observable: true,
            },
            ReconciliationDecision::Settle,
        ),
        (
            DetachObservation {
                dispatched: true,
                source_matches: true,
                destination_matches: false,
                conflicting_state: false,
                observable: true,
            },
            ReconciliationDecision::RetainUnknownFence,
        ),
        (
            DetachObservation {
                dispatched: true,
                source_matches: false,
                destination_matches: false,
                conflicting_state: false,
                observable: false,
            },
            ReconciliationDecision::RetainUnknownFence,
        ),
    ];

    for (observation, expected) in cases {
        assert_eq!(reconcile_detach(observation), expected);
    }
}

#[test]
fn record_reconciliation_never_retries_unknown_dispatch() {
    assert_eq!(
        reconcile_record(RecordObservation::NotDispatched),
        ReconciliationDecision::AbortAndClearFence
    );
    assert_eq!(
        reconcile_record(RecordObservation::IntendedCanonicalBytesPresent),
        ReconciliationDecision::Settle
    );
    for observation in [
        RecordObservation::IntendedTargetAbsentAfterDispatch,
        RecordObservation::ConflictingRecordOrGeneration,
        RecordObservation::Unavailable,
    ] {
        assert_eq!(
            reconcile_record(observation),
            ReconciliationDecision::RetainUnknownFence
        );
    }
}

#[cfg(unix)]
#[test]
fn exclusive_lock_refuses_a_racing_holder() {
    let directory = tempfile::tempdir().unwrap();
    let parent = std::fs::File::open(directory.path()).unwrap();
    let path = directory.path().join("scope.lock");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let mut permissions = file.metadata().unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o600);
    file.set_permissions(permissions).unwrap();

    let barrier = Arc::new(Barrier::new(2));
    let holder_path = path.clone();
    let holder_barrier = Arc::clone(&barrier);
    let holder = thread::spawn(move || {
        let parent = std::fs::File::open(holder_path.parent().unwrap()).unwrap();
        let lock =
            ProtocolLock::acquire_at(&parent, b"scope.lock", LockMode::Exclusive, false, false)
                .unwrap();
        holder_barrier.wait();
        holder_barrier.wait();
        drop(lock);
    });

    barrier.wait();
    assert!(
        ProtocolLock::acquire_at(&parent, b"scope.lock", LockMode::Shared, false, true).is_err()
    );
    assert!(
        ProtocolLock::acquire_at(&parent, b"scope.lock", LockMode::Exclusive, false, true).is_err()
    );
    barrier.wait();
    holder.join().unwrap();
    assert!(
        ProtocolLock::acquire_at(&parent, b"scope.lock", LockMode::Shared, false, true).is_ok()
    );
}

#[cfg(unix)]
#[test]
fn exclusive_lock_refuses_a_racing_process() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("process.lock");
    let parent = std::fs::File::open(directory.path()).unwrap();
    let held = ProtocolLock::acquire_at(&parent, b"process.lock", LockMode::Exclusive, true, false)
        .unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("lock_child_helper")
        .env("OMEGON_MAINTENANCE_LOCK_TEST", &path)
        .status()
        .unwrap();
    drop(held);
    assert!(status.success());
}

#[cfg(unix)]
#[test]
fn lock_child_helper() {
    let Ok(path) = std::env::var("OMEGON_MAINTENANCE_LOCK_TEST") else {
        return;
    };
    let path = std::path::Path::new(&path);
    let parent = std::fs::File::open(path.parent().unwrap()).unwrap();
    assert!(
        ProtocolLock::acquire_at(
            &parent,
            path.file_name().unwrap().as_encoded_bytes(),
            LockMode::Shared,
            false,
            true,
        )
        .is_err(),
        "child unexpectedly acquired a shared lock"
    );
}

#[cfg(unix)]
#[test]
fn contribution_admission_initializes_state_and_holds_shared_lock() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        path_identity(&home).unwrap(),
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let contributions = tempfile::tempdir().unwrap();
    let parent = path_identity(&File::open(contributions.path()).unwrap()).unwrap();
    let authority = scope_key(ContributionKind::Workflow.as_str(), "project", parent.key);

    let first = state
        .admit_contribution_scope(
            ContributionKind::Workflow,
            "project",
            &parent,
            "test-first",
            false,
        )
        .unwrap();
    assert_eq!(first.scope_key, authority);
    assert_eq!(first.generation, 0);
    assert!(first.allows(b"build.toml").unwrap());

    let second = state
        .admit_contribution_scope(
            ContributionKind::Workflow,
            "project",
            &parent,
            "test-second",
            true,
        )
        .unwrap();
    let lock_name = format!("contribution-{authority}.lock");
    assert!(
        ProtocolLock::acquire_at(
            &state.locks,
            lock_name.as_bytes(),
            LockMode::Exclusive,
            false,
            true,
        )
        .is_err()
    );
    drop((first, second));

    let deny_directory = open_secure_dir_at(&state.deny, authority.to_hex().as_bytes())
        .unwrap()
        .unwrap();
    let deny: DenyStateV1 = read_record_at(&deny_directory, b"state.json")
        .unwrap()
        .unwrap();
    assert_eq!(deny.scope_key, authority);
    assert_eq!(deny.generation, 0);
    assert!(deny.entries.is_empty());
}

#[cfg(unix)]
#[test]
fn contribution_mutation_initializes_state_and_holds_exclusive_lock() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        path_identity(&home).unwrap(),
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let contributions = tempfile::tempdir().unwrap();
    let parent = path_identity(&File::open(contributions.path()).unwrap()).unwrap();
    let kind = ContributionKind::Skill;
    let authority = scope_key(kind.as_str(), "project", parent.key);

    let mutation = state
        .lock_contribution_scope_mutation(kind, "project", &parent, "mutation", false)
        .unwrap();
    assert_eq!(mutation.scope_key, authority);
    assert_eq!(mutation.generation, 0);
    assert!(
        state
            .admit_contribution_scope(kind, "project", &parent, "blocked-reader", true)
            .is_err()
    );
    drop(mutation);

    let admission = state
        .admit_contribution_scope(kind, "project", &parent, "reader", true)
        .unwrap();
    assert_eq!(admission.scope_key, authority);
    assert_eq!(admission.generation, 0);
}

#[cfg(unix)]
#[test]
fn contribution_admission_matches_deny_to_exact_raw_name() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        path_identity(&home).unwrap(),
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let contributions = tempfile::tempdir().unwrap();
    let parent = path_identity(&File::open(contributions.path()).unwrap()).unwrap();
    let kind = ContributionKind::Workflow;
    let authority = scope_key(kind.as_str(), "project", parent.key);
    drop(
        state
            .admit_contribution_scope(kind, "project", &parent, "initialize", false)
            .unwrap(),
    );
    let deny_directory = open_secure_dir_at(&state.deny, authority.to_hex().as_bytes())
        .unwrap()
        .unwrap();
    let raw_name = b"build.toml";
    let denied_entry = entry_key(kind.as_str(), authority, raw_name);
    let request_id = "00000000-0000-0000-0000-000000000001";
    let record = DenyRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "deny".into(),
        record_id: derive_key(
            "deny",
            &[
                authority.as_bytes(),
                denied_entry.as_bytes(),
                request_id.as_bytes(),
            ],
        ),
        scope_key: authority,
        contribution_kind: kind,
        entry_key: denied_entry,
        raw_name_digest: AuthorityKey::from_bytes(Sha256::digest(raw_name).into()),
        generation: 1,
        state: DenyState::Denied,
        request_id: request_id.into(),
        created_at: "2026-08-17T00:00:00Z".into(),
    };
    let mut deny = DenyStateV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "deny_state".into(),
        record_id: derive_key("deny-state", &[authority.as_bytes(), &1_u64.to_be_bytes()]),
        scope_key: authority,
        generation: 1,
        entries: [(denied_entry.to_hex(), record)].into(),
    };
    replace_record_at(&deny_directory, b"state.json", &deny, "deny").unwrap();

    let guard = state
        .admit_contribution_scope(kind, "project", &parent, "read", false)
        .unwrap();
    assert_eq!(guard.generation, 1);
    assert!(!guard.allows(raw_name).unwrap());
    assert!(guard.allows(b"Build.toml").unwrap());
    drop(guard);

    deny.entries
        .get_mut(&denied_entry.to_hex())
        .unwrap()
        .raw_name_digest = AuthorityKey::from_bytes(Sha256::digest(b"other.toml").into());
    replace_record_at(&deny_directory, b"state.json", &deny, "forged-digest").unwrap();
    let guard = state
        .admit_contribution_scope(kind, "project", &parent, "read-forged", false)
        .unwrap();
    assert!(guard.allows(raw_name).is_err());
}

#[cfg(unix)]
#[test]
fn contribution_admission_rejects_fence_and_malformed_state() {
    use std::{fs::File, io::Write};

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        path_identity(&home).unwrap(),
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let contributions = tempfile::tempdir().unwrap();
    let parent = path_identity(&File::open(contributions.path()).unwrap()).unwrap();
    let kind = ContributionKind::Workflow;
    let authority = scope_key(kind.as_str(), "project", parent.key);
    drop(
        state
            .admit_contribution_scope(kind, "project", &parent, "initialize", false)
            .unwrap(),
    );
    let domain = contribution_domain_key(authority);
    let transaction = derive_key("transaction-test", &[b"test"]);
    let fence = FenceV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "fence".into(),
        record_id: derive_key("fence", &[domain.as_bytes(), transaction.as_bytes()]),
        domain_key: domain,
        transaction_record_id: transaction,
        state: FenceState::Active,
    };
    let fence_name = format!("{domain}.json");
    create_record_no_replace_at(&state.fences, fence_name.as_bytes(), &fence, "fence").unwrap();
    assert!(
        state
            .admit_contribution_scope(kind, "project", &parent, "fenced", false)
            .is_err()
    );
    assert!(
        state
            .lock_contribution_scope_mutation(kind, "project", &parent, "fenced-write", false)
            .is_err()
    );
    std::fs::remove_file(directory.path().join("maintain/v1/fences").join(fence_name)).unwrap();

    let state_path = directory
        .path()
        .join("maintain/v1/deny")
        .join(authority.to_hex())
        .join("state.json");
    let mut malformed = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(state_path)
        .unwrap();
    malformed.write_all(b"{not-json").unwrap();
    malformed.sync_all().unwrap();
    assert!(
        state
            .admit_contribution_scope(kind, "project", &parent, "malformed", false)
            .is_err()
    );
    assert!(
        state
            .lock_contribution_scope_mutation(kind, "project", &parent, "malformed-write", false)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn session_resume_admission_holds_shared_lock_and_rejects_deny() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        identity,
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let workspace = workspace_key("unix", b"/tmp/workspace");
    let session_id = "2026-08-17T00-00-00_00000000";
    let authority = session_key(session_id, workspace);
    let guard = state
        .admit_session_resume(session_id, workspace, false)
        .unwrap();
    assert_eq!(guard.session_key, authority);
    let lock_name = format!("session-{authority}.lock");
    assert!(
        ProtocolLock::acquire_at(
            &state.locks,
            lock_name.as_bytes(),
            LockMode::Exclusive,
            false,
            true,
        )
        .is_err()
    );
    drop(guard);

    let request_id = "00000000-0000-0000-0000-000000000001";
    let deny = SessionDenyRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "session_deny".into(),
        record_id: derive_key(
            "session-deny",
            &[authority.as_bytes(), request_id.as_bytes()],
        ),
        session_key: authority,
        session_id: session_id.into(),
        workspace_key: workspace,
        state: SessionDenyState::ResumeDenied,
        request_id: request_id.into(),
        created_at: "2026-08-17T00:00:00Z".into(),
    };
    let deny_name = format!("{authority}.json");
    create_record_no_replace_at(&state.session_deny, deny_name.as_bytes(), &deny, "test").unwrap();
    assert!(matches!(
        state.admit_session_resume(session_id, workspace, false),
        Err(omegon_maintenance_contracts::ContractError::SessionResumeDenied)
    ));
}

#[cfg(unix)]
#[test]
fn session_resume_admission_fails_closed_on_malformed_deny() {
    use std::{fs::File, io::Write, os::unix::fs::OpenOptionsExt};

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        identity,
        "11111111-1111-1111-1111-111111111111",
        false,
    )
    .unwrap();
    let workspace = workspace_key("unix", b"/tmp/workspace");
    let session_id = "2026-08-17T00-00-00_00000000";
    let authority = session_key(session_id, workspace);
    let deny_name = format!("{authority}.json");
    let deny_path = directory
        .path()
        .join("maintain/v1/session-deny")
        .join(deny_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(deny_path)
        .unwrap();
    file.write_all(b"{not-json").unwrap();
    assert!(
        state
            .admit_session_resume(session_id, workspace, false)
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn secure_root_admission_rejects_relative_root_and_final_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    assert!(open_secure_root(directory.path()).is_ok());
    assert!(open_secure_root(std::path::Path::new("relative")).is_err());
    assert!(open_secure_root(std::path::Path::new("/")).is_err());
    let alias = directory.path().with_extension("alias");
    symlink(directory.path(), &alias).unwrap();
    assert!(open_secure_root(&alias).is_err());
}

#[cfg(unix)]
#[test]
fn lock_rejects_symlinks_and_permissive_modes() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target.lock");
    let link = directory.path().join("link.lock");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .unwrap();
    let mut permissions = file.metadata().unwrap().permissions();
    permissions.set_mode(0o644);
    file.set_permissions(permissions).unwrap();
    symlink(&target, &link).unwrap();
    let parent = std::fs::File::open(directory.path()).unwrap();

    assert!(
        ProtocolLock::acquire_at(&parent, b"target.lock", LockMode::Exclusive, false, true)
            .is_err()
    );
    assert!(
        ProtocolLock::acquire_at(&parent, b"link.lock", LockMode::Exclusive, false, true).is_err()
    );
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_is_race_safe_and_home_bound() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for candidate in [
        "11111111-1111-1111-1111-111111111111",
        "22222222-2222-2222-2222-222222222222",
    ] {
        let home = home.try_clone().unwrap();
        let identity = identity.clone();
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            barrier.wait();
            MaintenanceStateV1::bootstrap(&home, identity, candidate, false)
                .unwrap()
                .installation
                .installation_uuid
        }));
    }
    barrier.wait();
    let identities: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(identities[0], identities[1]);

    for relative in [
        "maintain/v1/state.json",
        "maintain/v1/locks/bootstrap.lock",
        "maintain/v1/locks/audit.lock",
        "maintain/v1/deny",
        "maintain/v1/session-deny",
        "maintain/v1/transactions",
        "maintain/v1/fences",
        "maintain/v1/audit/checkpoint.json",
        "maintain/v1/audit/frontier.json",
        "maintain/v1/audit/receipts",
        "maintain/v1/audit/segments",
    ] {
        assert!(
            directory.path().join(relative).exists(),
            "missing {relative}"
        );
    }
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_is_nonblocking_when_audit_lock_is_held() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let candidate = "11111111-1111-1111-1111-111111111111";
    let state = MaintenanceStateV1::bootstrap(&home, identity.clone(), candidate, false).unwrap();
    let _audit_lock = ProtocolLock::acquire_at(
        &state.locks,
        b"audit.lock",
        LockMode::Exclusive,
        false,
        false,
    )
    .unwrap();

    assert!(MaintenanceStateV1::bootstrap(&home, identity, candidate, true).is_err());
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_rejects_symlink_components() {
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
    };

    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), directory.path().join("maintain")).unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    assert!(
        MaintenanceStateV1::bootstrap(
            &home,
            identity,
            "11111111-1111-1111-1111-111111111111",
            false,
        )
        .is_err()
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_repairs_durable_audit_tail() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let candidate = "11111111-1111-1111-1111-111111111111";
    let state = MaintenanceStateV1::bootstrap(&home, identity.clone(), candidate, false).unwrap();
    let record = AuditRecordV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit".into(),
        record_id: derive_key(
            "audit",
            &[
                state.installation.installation_uuid.as_bytes(),
                &1_u64.to_be_bytes(),
            ],
        ),
        installation_uuid: state.installation.installation_uuid.clone(),
        sequence: 1,
        previous_digest: None,
        request_id: "00000000-0000-0000-0000-000000000001".into(),
        command: "crash.fixture".into(),
        outcome: ResultStatus::Success,
    };
    append_bytes_at(
        &state.audit_segments,
        b"1.jsonl",
        &canonical_json(&record).unwrap(),
    )
    .unwrap();
    append_bytes_at(&state.audit_segments, b"1.jsonl", b"partial").unwrap();
    drop(state);

    let repaired =
        MaintenanceStateV1::bootstrap(&home, identity.clone(), candidate, false).unwrap();
    assert_eq!(repaired.installation.next_audit_sequence, 2);
    let checkpoint: AuditCheckpointV1 = read_record_at(&repaired.audit, b"checkpoint.json")
        .unwrap()
        .unwrap();
    assert_eq!(checkpoint.last_sequence, 1);
    assert_eq!(checkpoint.last_digest, canonical_digest(&record).unwrap());
    let receipt_name = format!("{}.json", record.request_id);
    let receipt: AuditReceiptV1 = read_record_at(&repaired.audit_receipts, receipt_name.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(receipt.command, record.command);
    assert_eq!(receipt.outcome, record.outcome);
    assert_eq!(receipt.sequence, record.sequence);
    assert_eq!(receipt.audit_digest, canonical_digest(&record).unwrap());
    assert_eq!(
        read_bytes_at(&repaired.audit_segments, b"1.jsonl", 1024)
            .unwrap()
            .unwrap(),
        canonical_json(&record).unwrap()
    );

    let receipt_identity =
        record_identity_at(&repaired.audit_receipts, receipt_name.as_bytes()).unwrap();
    drop(repaired);
    let repaired_again = MaintenanceStateV1::bootstrap(&home, identity, candidate, false).unwrap();
    assert_eq!(
        record_identity_at(&repaired_again.audit_receipts, receipt_name.as_bytes()).unwrap(),
        receipt_identity,
        "a repaired tail receipt must not be rewritten on later bootstrap"
    );
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_does_not_scan_segments_beyond_its_frontier() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let candidate = "11111111-1111-1111-1111-111111111111";
    let state = MaintenanceStateV1::bootstrap(&home, identity.clone(), candidate, false).unwrap();
    append_bytes_at(&state.audit_segments, b"100001.jsonl", b"not-json\n").unwrap();
    drop(state);

    let state = MaintenanceStateV1::bootstrap(&home, identity, candidate, false).unwrap();
    assert_eq!(state.installation.next_audit_sequence, 1);
}

#[cfg(unix)]
#[test]
fn maintenance_state_bootstrap_rejects_unauthenticated_rotated_boundary() {
    use std::fs::File;

    let directory = tempfile::tempdir().unwrap();
    let home = File::open(directory.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let candidate = "11111111-1111-1111-1111-111111111111";
    let state = MaintenanceStateV1::bootstrap(&home, identity.clone(), candidate, false).unwrap();
    let digest = AuthorityKey::from_bytes([7; 32]);
    let checkpoint = AuditCheckpointV1 {
        schema_version: SCHEMA_VERSION,
        record_kind: "audit_checkpoint".into(),
        record_id: derive_key(
            "audit-checkpoint",
            &[
                candidate.as_bytes(),
                &100_000_u64.to_be_bytes(),
                digest.as_bytes(),
            ],
        ),
        installation_uuid: candidate.into(),
        last_sequence: 100_000,
        last_digest: digest,
    };
    replace_record_at(&state.audit, b"checkpoint.json", &checkpoint, candidate).unwrap();
    let mut installation = state.installation.clone();
    installation.next_audit_sequence = 100_001;
    replace_record_at(&state.root, b"state.json", &installation, candidate).unwrap();
    state.prepare_audit_segment(100_001, candidate).unwrap();
    drop(state);

    assert!(MaintenanceStateV1::bootstrap(&home, identity, candidate, false).is_err());
}

#[test]
fn home_recovery_pending_blocks_bootstrap_and_cached_admission() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let home = open_secure_root(temporary.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        identity.clone(),
        "11111111-1111-1111-1111-111111111111",
        true,
    )
    .unwrap();
    // Even malformed recovery evidence must fail closed, not be ignored.
    let journal = temporary.path().join("maintain/v1/home-recovery.json");
    std::fs::write(&journal, b"{\"phase\":\"prepared\"}\n").unwrap();
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        MaintenanceStateV1::bootstrap(
            &home,
            identity.clone(),
            "22222222-2222-2222-2222-222222222222",
            true,
        )
        .is_err(),
        "pending recovery must block new bootstrap"
    );
    assert!(
        state
            .admit_contribution_scope(
                ContributionKind::Skill,
                "user",
                &identity,
                "pending-test",
                true,
            )
            .is_err(),
        "cached state must not admit contributions through pending recovery"
    );
    assert!(
        state
            .admit_session_resume("fixture", identity.key, true)
            .is_err(),
        "cached state must not admit a session through pending recovery"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn home_recovery_stable_volume_accepts_device_renumbering_not_directory_replacement() {
    use omegon_maintenance_contracts::{HomeContinuityV1, stable_home_volume_uuid};
    let temporary = tempfile::tempdir().unwrap();
    let home = open_secure_root(temporary.path()).unwrap();
    let identity = path_identity(&home).unwrap();
    let state = MaintenanceStateV1::bootstrap(
        &home,
        identity.clone(),
        "11111111-1111-1111-1111-111111111111",
        true,
    )
    .unwrap();
    let binding: HomeContinuityV1 = read_record_at(&state.root, b"home-continuity.json")
        .unwrap()
        .unwrap();
    assert_eq!(
        Some(binding.volume_uuid.clone()),
        stable_home_volume_uuid(&home).unwrap()
    );
    let mut renumbered = identity.clone();
    renumbered.device ^= 2;
    assert!(
        MaintenanceStateV1::bootstrap(
            &home,
            renumbered.clone(),
            "11111111-1111-1111-1111-111111111111",
            true
        )
        .is_ok()
    );
    renumbered.inode += 1;
    assert!(
        MaintenanceStateV1::bootstrap(
            &home,
            renumbered,
            "11111111-1111-1111-1111-111111111111",
            true
        )
        .is_err()
    );
    let mut changed_volume = binding;
    changed_volume.volume_uuid = "11111111111111111111111111111111".into();
    replace_record_at(
        &state.root,
        b"home-continuity.json",
        &changed_volume,
        "test",
    )
    .unwrap();
    assert!(
        MaintenanceStateV1::bootstrap(
            &home,
            identity,
            "11111111-1111-1111-1111-111111111111",
            true
        )
        .is_err()
    );
}
