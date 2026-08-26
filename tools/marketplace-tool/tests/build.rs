use std::{
    fs::{self, Permissions},
    os::unix::fs::PermissionsExt as _,
    path::Path,
    process::Command,
};

use ring::signature::{Ed25519KeyPair, KeyPair as _};

const FIXED_GENERATED: &str = "2026-08-25T00:00:00Z";
const FIXED_EXPIRES: &str = "2036-08-25T00:00:00Z";

#[test]
fn cli_build_is_reproducible_and_verifies_its_complete_source() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    let target = tempfile::tempdir().expect("isolated cargo target");
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "wasm32-wasip2",
            "-p",
            "hello-widget",
            "--target-dir",
        ])
        .arg(target.path())
        .status()
        .expect("build hello fixture");
    assert!(status.success(), "hello fixture must build");
    prepare_fixture(
        fixture.path(),
        &target
            .path()
            .join("wasm32-wasip2/release/hello_widget.wasm"),
    );

    let build = || {
        Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
            .args([
                "build",
                "--repository",
                fixture.path().to_str().expect("UTF-8 fixture path"),
                "--generated-at",
                FIXED_GENERATED,
                "--expires-at",
                FIXED_EXPIRES,
                "--development-key",
            ])
            .status()
            .expect("run marketplace build")
    };
    assert!(build().success(), "first build");
    assert_eq!(
        fs::read(
            fixture
                .path()
                .join("marketplace/development-catalog-state.json")
        )
        .expect("development state"),
        include_bytes!("../../../marketplace/development-catalog-state.json")
    );
    let catalog_path = fixture.path().join("public/marketplace/v1/catalog.json");
    let first = fs::read(&catalog_path).expect("first catalog");
    fs::remove_file(&catalog_path).expect("simulate interruption after durable state");
    assert!(build().success(), "retry after state commit");
    assert_eq!(
        fs::read(&catalog_path).expect("retried catalog"),
        first,
        "catalog.json is the only authority commit point"
    );
    assert!(build().success(), "deterministic retry");
    assert_eq!(fs::read(&catalog_path).expect("second catalog"), first);

    let verified = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("verify")
        .arg("public/marketplace/v1/catalog.json")
        .current_dir(fixture.path())
        .status()
        .expect("verify catalog");
    assert!(verified.success(), "catalog signature and payload");

    let secrets = tempfile::tempdir().expect("production secrets");
    let counter = secrets.path().join("sequence.txt");
    let state = secrets.path().join("state.json");
    let signing_key = secrets.path().join("signing.key");
    let public_key = secrets.path().join("signing.pub");
    let seed = [42; 32];
    write_private(&counter, b"1\n");
    write_private(&signing_key, format!("{}\n", lower_hex(&seed)).as_bytes());
    let pair = Ed25519KeyPair::from_seed_unchecked(&seed).expect("production fixture key");
    fs::write(
        &public_key,
        format!("{}\n", lower_hex(pair.public_key().as_ref())),
    )
    .expect("public key fixture");
    let production = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .args([
            "build",
            "--repository",
            fixture.path().to_str().expect("UTF-8 fixture path"),
            "--generated-at",
            FIXED_GENERATED,
            "--expires-at",
            FIXED_EXPIRES,
            "--production",
            "--sequence-file",
            counter.to_str().expect("counter path"),
            "--sequence-state",
            state.to_str().expect("state path"),
            "--signing-key",
            signing_key.to_str().expect("key path"),
            "--key-id",
            "overcrow-production-test",
        ])
        .status()
        .expect("production build");
    assert!(production.success(), "isolated production build");
    assert_eq!(
        fs::metadata(&state)
            .expect("production state")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .arg("verify")
        .arg(&catalog_path)
        .arg("--public-key")
        .arg(&public_key)
        .arg("--key-id")
        .arg("overcrow-production-test")
        .status()
        .expect("verify production catalog");
    assert!(verified.success(), "production catalog verification");
}

fn write_private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("private fixture");
    fs::set_permissions(path, Permissions::from_mode(0o600)).expect("private fixture mode");
}

fn lower_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn prepare_fixture(repository: &Path, component: &Path) {
    let source = repository.join("examples/hello-widget");
    fs::create_dir_all(source.join("locales")).expect("source directories");
    fs::create_dir_all(repository.join("marketplace")).expect("marketplace directory");
    fs::create_dir_all(repository.join("fixtures/keys")).expect("key directory");
    fs::copy(component, source.join("component.wasm")).expect("component fixture");
    for relative in [
        "manifest.json",
        "listing.json",
        "preview.png",
        "locales/en.json",
        "locales/fr.json",
    ] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/hello-widget")
                .join(relative),
            source.join(relative),
        )
        .expect("source fixture");
    }
    for relative in [
        "marketplace/targets.json",
        "marketplace/development-sequence.txt",
        "fixtures/keys/development-ed25519.key",
    ] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(relative),
            repository.join(relative),
        )
        .expect("publisher fixture");
    }
}
