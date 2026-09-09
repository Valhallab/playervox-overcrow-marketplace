use super::*;
use crate::{admission, package};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair as _, UnparsedPublicKey};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
};

const TREE: &str = "3333333333333333333333333333333333333333";
const TRUST: &str = "1111111111111111111111111111111111111111";
const REVIEW: &str = "2222222222222222222222222222222222222222";

#[test]
fn detached_production_flow_preserves_admitted_bytes_and_is_idempotent() {
    let fixture = Fixture::new();
    assert_eq!(fixture.prepare(), 1);
    let payload = fs::read(fixture.prepared.join("payload.json")).unwrap();
    let parsed: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(parsed["sequence"], 43);
    assert_eq!(parsed["expiresAt"], "2026-12-04T12:00:00Z");
    assert!(
        parsed["targets"][0]["packageUrl"]
            .as_str()
            .unwrap()
            .starts_with("https://overcrow.playervox.com/marketplace/v1/packages/")
    );
    assert!(!fixture.prepared.join("catalog.json").exists());
    assert_eq!(fixture.prepare(), 1);
    let signature = fixture.sign();
    let options = fixture.finalize_options(&signature);
    assert_eq!(
        finalize_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap(),
        1
    );
    assert_eq!(
        finalize_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap(),
        1
    );
    let envelope: Value =
        serde_json::from_slice(&fs::read(fixture.output.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(envelope["keyId"], "overcrow-production-2026-01");
    let signed_payload = URL_SAFE_NO_PAD
        .decode(envelope["payload"].as_str().unwrap())
        .unwrap();
    assert_eq!(signed_payload, payload);
    UnparsedPublicKey::new(&ED25519, fixture.key.public_key().as_ref())
        .verify(&payload, &fs::read(signature).unwrap())
        .unwrap();
    let target = &parsed["targets"][0];
    let relative = target["packageUrl"]
        .as_str()
        .unwrap()
        .strip_prefix("https://overcrow.playervox.com/marketplace/v1/")
        .unwrap();
    assert_eq!(
        fs::read(fixture.output.join(relative)).unwrap(),
        fixture.package_bytes
    );
}

#[test]
fn production_entrypoint_rejects_test_signatures_and_has_no_key_override() {
    let fixture = Fixture::new();
    fixture.prepare();
    let signature = fixture.sign();
    assert_ne!(
        compiled_key().unwrap().as_slice(),
        fixture.key.public_key().as_ref()
    );
    assert!(finalize(&fixture.finalize_options(&signature)).is_err());
    assert!(!fixture.output.join("catalog.json").exists());
    assert!(
        read_reservation(&fixture.state)
            .unwrap()
            .unwrap()
            .completed_catalog_sha256
            .is_none()
    );
}

#[test]
fn finalize_rejects_tampering_without_a_visible_catalog() {
    for kind in [
        "payload",
        "signature",
        "package",
        "marker",
        "extra",
        "symlink",
    ] {
        let fixture = Fixture::new();
        fixture.prepare();
        let signature = fixture.sign();
        let payload: Payload =
            read_json(&fixture.prepared.join("payload.json"), MAX_PAYLOAD).unwrap();
        match kind {
            "payload" => {
                let mut bytes = fs::read(fixture.prepared.join("payload.json")).unwrap();
                bytes.push(b' ');
                fs::write(fixture.prepared.join("payload.json"), bytes).unwrap();
            }
            "signature" => fs::write(&signature, [0; 64]).unwrap(),
            "package" => fs::write(
                fixture
                    .prepared
                    .join(relative_package(&payload.targets[0]).unwrap()),
                b"tampered",
            )
            .unwrap(),
            "marker" => fs::write(fixture.prepared.join("preparation.json"), b"{}").unwrap(),
            "extra" => fs::write(fixture.prepared.join("extra"), b"unadmitted").unwrap(),
            _ => {
                let package = fixture
                    .prepared
                    .join(relative_package(&payload.targets[0]).unwrap());
                let outside = fixture.scratch.path().join("outside.ocpkg");
                fs::rename(&package, &outside).unwrap();
                std::os::unix::fs::symlink(outside, package).unwrap();
            }
        }
        assert!(
            finalize_inner(
                &fixture.finalize_options(&signature),
                fixture.key.public_key().as_ref(),
                now()
            )
            .is_err(),
            "{kind}"
        );
        assert!(!fixture.output.join("catalog.json").exists(), "{kind}");
    }
}

#[test]
fn prepare_rejects_invalid_time_status_and_sequence_requests_without_reserving() {
    for kind in [
        "future",
        "expired",
        "timestamp",
        "sequence",
        "overflow",
        "unknown",
        "duplicate",
        "limit",
        "expiresAt",
    ] {
        let fixture = Fixture::new();
        let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
        let change =
            json!({"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"revoked"});
        match kind {
            "future" => request["generatedAt"] = json!("2026-09-06T00:00:00Z"),
            "expired" => request["generatedAt"] = json!("2026-01-01T00:00:00Z"),
            "timestamp" => request["generatedAt"] = json!("2026-09-05T12:00:00+00:00"),
            "sequence" => request["sequence"] = json!(42),
            "overflow" => {
                request["previousSequence"] = json!(u64::MAX);
                request["sequence"] = json!(0);
            }
            "unknown" => {
                request["statuses"] =
                    json!([{"id":"com.example.unknown","version":"1.0.0","status":"revoked"}])
            }
            "duplicate" => request["statuses"] = json!([change.clone(), change]),
            "limit" => request["statuses"] = Value::Array(vec![change; 501]),
            _ => request["expiresAt"] = json!("2026-12-04T12:00:00Z"),
        }
        write_json(&fixture.request, &request);
        assert!(
            prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err(),
            "{kind}"
        );
        assert!(!fixture.state.join("state.json").exists(), "{kind}");
        assert!(
            !fixture.prepared.join("preparation.json").exists(),
            "{kind}"
        );
    }
}

#[test]
fn reservation_rejects_busy_conflicting_and_unfinished_successors() {
    let fixture = Fixture::new();
    let lock = private_fs::lock_private_directory(&fixture.state).unwrap();
    assert!(prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err());
    drop(lock);
    fixture.prepare();
    let original = fs::read(fixture.state.join("state.json")).unwrap();
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["statuses"] =
        json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"revoked"}]);
    write_json(&fixture.request, &request);
    assert!(prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err());
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    write_json(&fixture.request, &request);
    assert!(prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err());
    assert_eq!(
        fs::read(fixture.state.join("state.json")).unwrap(),
        original
    );
}

#[test]
fn expired_reservation_can_advance_but_old_signature_cannot_finalize() {
    let fixture = Fixture::new();
    fixture.prepare();
    let signature = fixture.sign();
    let later = now() + TimeDelta::days(91);
    assert!(
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            later
        )
        .is_err()
    );
    let prepared = directory(fixture.scratch.path(), "next-prepared");
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    request["generatedAt"] = json!(later.to_rfc3339_opts(SecondsFormat::Secs, true));
    write_json(&fixture.request, &request);
    let options = PrepareOptions {
        output: &prepared,
        ..fixture.options()
    };
    prepare_inner(&options, fixture.key.public_key().as_ref(), later).unwrap();
    assert!(
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            now()
        )
        .is_err()
    );
    assert_eq!(
        read_reservation(&fixture.state).unwrap().unwrap().sequence,
        44
    );
}

#[test]
fn production_retains_revoked_versions_and_exact_archives_across_updates() {
    let fixture = Fixture::new();
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["statuses"] =
        json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"revoked"}]);
    write_json(&fixture.request, &request);
    fixture.prepare();
    let signature = fixture.sign();
    finalize_inner(
        &fixture.finalize_options(&signature),
        fixture.key.public_key().as_ref(),
        now(),
    )
    .unwrap();
    let tree = "4444444444444444444444444444444444444444";
    let new_bytes = admit_version(
        fixture.scratch.path(),
        &fixture.store,
        tree,
        "2.0.0",
        b"<!doctype html><p>new</p>",
    );
    let prepared = directory(fixture.scratch.path(), "prepared2");
    let output = directory(fixture.scratch.path(), "output2");
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    request["statuses"] = json!([]);
    write_json(&fixture.request, &request);
    let options = PrepareOptions {
        review_tree: tree,
        output: &prepared,
        previous_output: Some(&fixture.output),
        ..fixture.options()
    };
    assert_eq!(
        prepare_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap(),
        2
    );
    let payload: Payload = read_json(&prepared.join("payload.json"), MAX_PAYLOAD).unwrap();
    assert_eq!(payload.targets.len(), 2);
    assert!(payload.targets[0].status == Status::Revoked);
    assert!(payload.targets[1].status == Status::Verified);
    assert_eq!(
        fs::read(prepared.join(relative_package(&payload.targets[0]).unwrap())).unwrap(),
        fixture.package_bytes
    );
    assert_eq!(
        fs::read(prepared.join(relative_package(&payload.targets[1]).unwrap())).unwrap(),
        new_bytes
    );
    request["statuses"] =
        json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"verified"}]);
    write_json(&fixture.request, &request);
    assert!(prepare_inner(&options, fixture.key.public_key().as_ref(), now()).is_err());
    let payload_bytes = fs::read(prepared.join("payload.json")).unwrap();
    fs::write(&signature, fixture.key.sign(&payload_bytes).as_ref()).unwrap();
    let finalize_options = FinalizeOptions {
        prepared: &prepared,
        state: &fixture.state,
        signature: &signature,
        output: &output,
    };
    finalize_inner(&finalize_options, fixture.key.public_key().as_ref(), now()).unwrap();
    assert!(
        output
            .join(relative_package(&payload.targets[0]).unwrap())
            .exists()
    );
    // The high-water mark forbids re-exporting an older valid signed preparation.
    assert!(
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            now()
        )
        .is_err()
    );
}

#[test]
fn explicit_removal_publishes_only_the_newer_admitted_version() {
    let fixture = Fixture::new();
    fixture.prepare();
    let signature = fixture.sign();
    finalize_inner(
        &fixture.finalize_options(&signature),
        fixture.key.public_key().as_ref(),
        now(),
    )
    .unwrap();
    let tree = "4444444444444444444444444444444444444444";
    let new_bytes = admit_version(
        fixture.scratch.path(),
        &fixture.store,
        tree,
        "2.0.0",
        b"<!doctype html><p>new</p>",
    );
    let prepared = directory(fixture.scratch.path(), "next-prepared");
    let output = directory(fixture.scratch.path(), "next-output");
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    request["removeVersions"] = json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0"}]);
    write_json(&fixture.request, &request);
    let options = PrepareOptions {
        review_tree: tree,
        output: &prepared,
        previous_output: Some(&fixture.output),
        ..fixture.options()
    };
    assert_eq!(
        prepare_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap(),
        1
    );
    let bytes = fs::read(prepared.join("payload.json")).unwrap();
    let payload: Payload = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(payload.targets[0].manifest["version"], "2.0.0");
    assert!(
        !prepared
            .join("packages/com.playervox.overcrow.hello/1.0.0")
            .exists()
    );
    fs::write(&signature, fixture.key.sign(&bytes).as_ref()).unwrap();
    let finalize_options = FinalizeOptions {
        prepared: &prepared,
        state: &fixture.state,
        signature: &signature,
        output: &output,
    };
    assert_eq!(
        finalize_inner(&finalize_options, fixture.key.public_key().as_ref(), now()).unwrap(),
        1
    );
    assert_eq!(
        fs::read(output.join(relative_package(&payload.targets[0]).unwrap())).unwrap(),
        new_bytes
    );
    assert!(
        !output
            .join("packages/com.playervox.overcrow.hello/1.0.0")
            .exists()
    );
    // The predecessor stays intact for recovery, but can no longer be finalized.
    assert!(fixture.output.join("catalog.json").exists());
    assert_eq!(
        read_reservation(&fixture.state).unwrap().unwrap().sequence,
        44
    );
    assert!(
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            now()
        )
        .is_err()
    );
}

#[test]
fn removal_rejects_security_history_and_missing_or_unverified_replacements() {
    for kind in [
        "revoked",
        "security-suspended",
        "unknown",
        "duplicate",
        "current",
        "no-newer",
        "blocked-newer",
        "status-conflict",
        "limit",
    ] {
        let fixture = Fixture::new();
        let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
        if matches!(kind, "revoked" | "security-suspended") {
            request["statuses"] =
                json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":kind}]);
            write_json(&fixture.request, &request);
        }
        fixture.prepare();
        let signature = fixture.sign();
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            now(),
        )
        .unwrap();
        let original_state = fs::read(fixture.state.join("state.json")).unwrap();
        let tree = if kind == "no-newer" {
            TREE
        } else {
            "4444444444444444444444444444444444444444"
        };
        if tree != TREE {
            admit_version(
                fixture.scratch.path(),
                &fixture.store,
                tree,
                "2.0.0",
                b"<!doctype html><p>new</p>",
            );
        }
        let prepared = directory(fixture.scratch.path(), "next-prepared");
        request["sequence"] = json!(44);
        request["previousSequence"] = json!(43);
        request["statuses"] = json!([]);
        let removal = json!({"id":"com.playervox.overcrow.hello","version":"1.0.0"});
        request["removeVersions"] = json!([removal.clone()]);
        match kind {
            "unknown" => request["removeVersions"][0]["version"] = json!("0.9.0"),
            "duplicate" => request["removeVersions"] = json!([removal.clone(), removal]),
            "current" => request["removeVersions"][0]["version"] = json!("2.0.0"),
            "blocked-newer" => {
                request["statuses"] = json!([{"id":"com.playervox.overcrow.hello","version":"2.0.0","status":"security-suspended"}])
            }
            "status-conflict" => {
                request["statuses"] = json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"verified"}])
            }
            "limit" => request["removeVersions"] = Value::Array(vec![removal; 501]),
            _ => {}
        }
        write_json(&fixture.request, &request);
        let options = PrepareOptions {
            review_tree: tree,
            output: &prepared,
            previous_output: Some(&fixture.output),
            ..fixture.options()
        };
        assert!(
            prepare_inner(&options, fixture.key.public_key().as_ref(), now()).is_err(),
            "{kind}"
        );
        assert_eq!(
            fs::read(fixture.state.join("state.json")).unwrap(),
            original_state,
            "{kind}"
        );
        assert!(!prepared.join("preparation.json").exists(), "{kind}");
    }
}

#[test]
fn previous_catalog_must_be_signed_by_production_authority_and_match_state() {
    let fixture = Fixture::new();
    fixture.prepare();
    let signature = fixture.sign();
    finalize_inner(
        &fixture.finalize_options(&signature),
        fixture.key.public_key().as_ref(),
        now(),
    )
    .unwrap();
    let prepared = directory(fixture.scratch.path(), "next");
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    write_json(&fixture.request, &request);
    let options = PrepareOptions {
        output: &prepared,
        previous_output: Some(&fixture.output),
        ..fixture.options()
    };
    let mut envelope: Value =
        read_json(&fixture.output.join("catalog.json"), MAX_ENVELOPE).unwrap();
    envelope["keyId"] = json!("overcrow-development-2026");
    write_json(&fixture.output.join("catalog.json"), &envelope);
    assert!(prepare_inner(&options, fixture.key.public_key().as_ref(), now()).is_err());
    envelope["keyId"] = json!(KEY_ID);
    envelope["signature"] = json!(URL_SAFE_NO_PAD.encode([0; 64]));
    write_json(&fixture.output.join("catalog.json"), &envelope);
    assert!(prepare_inner(&options, fixture.key.public_key().as_ref(), now()).is_err());
    let without_previous = PrepareOptions {
        previous_output: None,
        ..options
    };
    assert!(prepare_inner(&without_previous, fixture.key.public_key().as_ref(), now()).is_err());
}

#[test]
fn production_refuses_repository_roots_and_unrelated_output_content() {
    let fixture = Fixture::new();
    fs::write(fixture.prepared.join("unrelated"), b"keep").unwrap();
    assert!(prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err());
    assert_eq!(
        fs::read(fixture.prepared.join("unrelated")).unwrap(),
        b"keep"
    );
    fs::remove_file(fixture.prepared.join("unrelated")).unwrap();
    fs::create_dir(fixture.scratch.path().join(".git")).unwrap();
    assert!(prepare_inner(&fixture.options(), fixture.key.public_key().as_ref(), now()).is_err());
    assert!(!fixture.state.join("state.json").exists());
}

#[test]
fn completed_signature_reservation_survives_catalog_commit_failure() {
    let fixture = Fixture::new();
    fixture.prepare();
    let signature = fixture.sign();
    fs::write(
        fixture.output.join("catalog.json"),
        b"unrelated existing bytes",
    )
    .unwrap();
    let options = fixture.finalize_options(&signature);
    assert!(finalize_inner(&options, fixture.key.public_key().as_ref(), now()).is_err());
    assert_eq!(
        fs::read(fixture.output.join("catalog.json")).unwrap(),
        b"unrelated existing bytes"
    );
    assert!(
        read_reservation(&fixture.state)
            .unwrap()
            .unwrap()
            .completed_catalog_sha256
            .is_some()
    );
    fs::remove_file(fixture.output.join("catalog.json")).unwrap();
    finalize_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap();
    let envelope: Envelope = read_json(&fixture.output.join("catalog.json"), MAX_ENVELOPE).unwrap();
    assert_eq!(envelope.key_id, KEY_ID);
}

#[test]
fn old_signed_versions_cannot_be_replaced_or_downgraded_by_another_admission_store() {
    for kind in ["replacement", "downgrade"] {
        let fixture = Fixture::new();
        let tree = "4444444444444444444444444444444444444444";
        if kind == "downgrade" {
            admit_version(
                fixture.scratch.path(),
                &fixture.store,
                tree,
                "2.0.0",
                b"<!doctype html><p>new</p>",
            );
        }
        let first = PrepareOptions {
            review_tree: if kind == "downgrade" { tree } else { TREE },
            ..fixture.options()
        };
        prepare_inner(&first, fixture.key.public_key().as_ref(), now()).unwrap();
        let signature = fixture.sign();
        finalize_inner(
            &fixture.finalize_options(&signature),
            fixture.key.public_key().as_ref(),
            now(),
        )
        .unwrap();
        let another_store = directory(fixture.scratch.path(), "another-store");
        let another_root = directory(fixture.scratch.path(), "another-input");
        admit_version(
            &another_root,
            &another_store,
            tree,
            "1.0.0",
            b"<!doctype html><p>replacement</p>",
        );
        let prepared = directory(fixture.scratch.path(), "next-prepared");
        let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
        request["sequence"] = json!(44);
        request["previousSequence"] = json!(43);
        write_json(&fixture.request, &request);
        let options = PrepareOptions {
            store: &another_store,
            review_tree: tree,
            output: &prepared,
            previous_output: Some(&fixture.output),
            ..fixture.options()
        };
        assert!(
            prepare_inner(&options, fixture.key.public_key().as_ref(), now()).is_err(),
            "{kind}"
        );
        assert_eq!(
            read_reservation(&fixture.state).unwrap().unwrap().sequence,
            43
        );
    }
}

#[test]
fn suspension_requires_an_explicit_change_to_be_lifted() {
    let fixture = Fixture::new();
    let mut request: Value = read_json(&fixture.request, MAX_REQUEST).unwrap();
    request["statuses"] = json!([{"id":"com.playervox.overcrow.hello","version":"1.0.0","status":"security-suspended"}]);
    write_json(&fixture.request, &request);
    fixture.prepare();
    let signature = fixture.sign();
    finalize_inner(
        &fixture.finalize_options(&signature),
        fixture.key.public_key().as_ref(),
        now(),
    )
    .unwrap();
    request["sequence"] = json!(44);
    request["previousSequence"] = json!(43);
    request["statuses"] = json!([]);
    write_json(&fixture.request, &request);
    let prepared = directory(fixture.scratch.path(), "next-prepared");
    let options = PrepareOptions {
        output: &prepared,
        previous_output: Some(&fixture.output),
        ..fixture.options()
    };
    prepare_inner(&options, fixture.key.public_key().as_ref(), now()).unwrap();
    let payload: Payload = read_json(&prepared.join("payload.json"), MAX_PAYLOAD).unwrap();
    assert!(payload.targets[0].status == Status::SecuritySuspended);
}

fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn admit_version(root: &Path, store: &Path, tree: &str, version: &str, view: &[u8]) -> Vec<u8> {
    let input = directory(root, &format!("input-{tree}"));
    let source = directory(&input, "source");
    let artifacts = directory(&input, "artifacts");
    fs::write(source.join("index.html"), view).unwrap();
    let listing = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello-web/listing.json"),
    )
    .unwrap();
    fs::write(source.join("listing.json"), &listing).unwrap();
    write_json(
        &source.join("manifest.json"),
        &json!({"schemaVersion":1,"id":"com.playervox.overcrow.hello","version":version,"apiVersion":"1","entrypoints":{"view":"index.html"},"permissions":{},"files":{"index.html":{"sha256":hash(view),"bytes":view.len()}}}),
    );
    let archive = artifacts.join("1.ocpkg");
    let written = package::write_package(&source, &archive).unwrap();
    let bytes = fs::read(archive).unwrap();
    fs::write(artifacts.join("1.listing.json"), &listing).unwrap();
    let receipt = input.join("receipt.tsv");
    fs::write(&receipt,format!("admission\t2\t{TRUST}\t{REVIEW}\t{tree}\nartifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t{version}\t{}\t{}\t{}\t{}\n",package::sha256_hex(&written.digest),bytes.len(),hash(&listing),listing.len())).unwrap();
    admission::ingest(
        &receipt,
        &artifacts,
        store,
        &admission::ExpectedAdmission {
            trust_sha: TRUST,
            review_sha: REVIEW,
            review_tree: tree,
        },
    )
    .unwrap();
    bytes
}

struct Fixture {
    scratch: tempfile::TempDir,
    store: PathBuf,
    state: PathBuf,
    request: PathBuf,
    prepared: PathBuf,
    output: PathBuf,
    package_bytes: Vec<u8>,
    key: Ed25519KeyPair,
}

impl Fixture {
    fn new() -> Self {
        let scratch = tempfile::tempdir_in("/var/tmp").unwrap();
        fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let store = directory(scratch.path(), "store");
        let state = directory(scratch.path(), "state");
        let prepared = directory(scratch.path(), "prepared");
        let output = directory(scratch.path(), "output");
        let artifacts = directory(scratch.path(), "artifacts");
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello-web");
        let package_path = artifacts.join("1.ocpkg");
        let written = package::write_package(&source, &package_path).unwrap();
        let package_bytes = fs::read(&package_path).unwrap();
        let listing = fs::read(source.join("listing.json")).unwrap();
        fs::write(artifacts.join("1.listing.json"), &listing).unwrap();
        let receipt = scratch.path().join("receipt.tsv");
        fs::write(&receipt, format!("admission\t2\t{TRUST}\t{REVIEW}\t{TREE}\nartifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t1.0.0\t{}\t{}\t{}\t{}\n",package::sha256_hex(&written.digest),package_bytes.len(),hash(&listing),listing.len())).unwrap();
        admission::ingest(
            &receipt,
            &artifacts,
            &store,
            &admission::ExpectedAdmission {
                trust_sha: TRUST,
                review_sha: REVIEW,
                review_tree: TREE,
            },
        )
        .unwrap();
        let request = scratch.path().join("request.json");
        fs::write(&request, serde_json::to_vec(&json!({"schemaVersion":1,"sequence":43,"previousSequence":42,"generatedAt":"2026-09-05T12:00:00Z","statuses":[]})).unwrap()).unwrap();
        let key = Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap();
        Self {
            scratch,
            store,
            state,
            request,
            prepared,
            output,
            package_bytes,
            key,
        }
    }
    fn options(&self) -> PrepareOptions<'_> {
        PrepareOptions {
            store: &self.store,
            review_tree: TREE,
            state: &self.state,
            request: &self.request,
            output: &self.prepared,
            previous_output: None,
        }
    }
    fn prepare(&self) -> usize {
        prepare_inner(&self.options(), self.key.public_key().as_ref(), now())
            .expect("prepare exact accepted bytes")
    }
    fn sign(&self) -> PathBuf {
        let signature = self.scratch.path().join("signature.bin");
        fs::write(
            &signature,
            self.key
                .sign(&fs::read(self.prepared.join("payload.json")).unwrap())
                .as_ref(),
        )
        .unwrap();
        signature
    }
    fn finalize_options<'a>(&'a self, signature: &'a Path) -> FinalizeOptions<'a> {
        FinalizeOptions {
            prepared: &self.prepared,
            state: &self.state,
            signature,
            output: &self.output,
        }
    }
}
fn directory(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-09-05T12:01:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc)
}
fn hash(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    package::sha256_hex(&Sha256::digest(bytes).into())
}
