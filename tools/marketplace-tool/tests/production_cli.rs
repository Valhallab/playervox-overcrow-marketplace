use std::process::Command;

#[test]
fn production_commands_reject_arguments_before_reading_inputs() {
    for command in ["prepare-production-catalog", "finalize-production-catalog"] {
        let output = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
            .args([command, "--signing-key", "/must-not-be-read"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "error: invalid production catalog arguments\n"
        );
    }
}

#[test]
fn production_cli_prepares_accepted_bytes_and_rejects_an_untrusted_signature() {
    use sha2::{Digest as _, Sha256};
    use std::{fs, os::unix::fs::PermissionsExt as _, path::Path};
    let scratch = tempfile::tempdir_in("/var/tmp").unwrap();
    fs::set_permissions(scratch.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let directory = |name| {
        let path = scratch.path().join(name);
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    };
    let artifacts = directory("artifacts");
    let store = directory("store");
    let state = directory("state");
    let prepared = directory("prepared");
    let output = directory("output");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello-web");
    let archive = artifacts.join("1.ocpkg");
    let package = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("package")
        .arg(&source)
        .arg(&archive)
        .output()
        .unwrap();
    assert!(package.status.success());
    let stdout = String::from_utf8(package.stdout).unwrap();
    let digest = stdout.split_whitespace().next().unwrap();
    let listing = fs::read(source.join("listing.json")).unwrap();
    fs::write(artifacts.join("1.listing.json"), &listing).unwrap();
    let listing_digest = Sha256::digest(&listing)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let receipt = scratch.path().join("receipt.tsv");
    let trust = "1".repeat(40);
    let review = "2".repeat(40);
    let tree = "3".repeat(40);
    fs::write(&receipt, format!("admission\t2\t{trust}\t{review}\t{tree}\nartifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t1.0.0\t{digest}\t{}\t{listing_digest}\t{}\n",fs::metadata(&archive).unwrap().len(),listing.len())).unwrap();
    let ingestion = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("ingest")
        .arg("--receipt")
        .arg(&receipt)
        .arg("--artifacts")
        .arg(&artifacts)
        .arg("--store")
        .arg(&store)
        .arg("--trust-sha")
        .arg(trust)
        .arg("--review-sha")
        .arg(review)
        .arg("--review-tree")
        .arg(&tree)
        .output()
        .unwrap();
    assert!(ingestion.status.success());
    let request = scratch.path().join("request.json");
    let generated_at = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    fs::write(&request, serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"sequence":1,"previousSequence":0,"generatedAt":generated_at,"statuses":[]})).unwrap()).unwrap();
    let preparation = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("prepare-production-catalog")
        .arg("--store")
        .arg(&store)
        .arg("--review-tree")
        .arg(&tree)
        .arg("--state")
        .arg(&state)
        .arg("--request")
        .arg(&request)
        .arg("--output")
        .arg(&prepared)
        .output()
        .unwrap();
    assert!(
        preparation.status.success(),
        "{}",
        String::from_utf8_lossy(&preparation.stderr)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&fs::read(prepared.join("payload.json")).unwrap()).unwrap();
    assert_eq!(payload["targets"][0]["packageSha256"], digest);
    assert!(
        payload["targets"][0]["packageUrl"]
            .as_str()
            .unwrap()
            .starts_with("https://overcrow.playervox.com/marketplace/v1/packages/")
    );
    assert!(!prepared.join("catalog.json").exists());
    let signature = scratch.path().join("signature.bin");
    fs::write(&signature, [0; 64]).unwrap();
    let finalization = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("finalize-production-catalog")
        .arg("--prepared")
        .arg(&prepared)
        .arg("--state")
        .arg(&state)
        .arg("--signature")
        .arg(signature)
        .arg("--output")
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(finalization.status.code(), Some(1));
    assert!(!output.join("catalog.json").exists());
}
