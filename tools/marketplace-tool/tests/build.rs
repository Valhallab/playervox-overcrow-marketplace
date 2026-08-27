use std::{
    fs::{self, Permissions},
    os::unix::fs::{PermissionsExt as _, symlink},
    path::Path,
    process::Command,
};

use image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::Value;

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

    let component = fixture.path().join("examples/hello-widget/component.wasm");

    let conflict_fixture = tempfile::tempdir().expect("fresh conflict repository");
    prepare_fixture(conflict_fixture.path(), &component);
    fs::write(
        conflict_fixture
            .path()
            .join("marketplace/development-catalog-state.json"),
        b"{\"schemaVersion\":1,\"sequence\":1,\"payloadSha256\":\"0000000000000000000000000000000000000000000000000000000000000000\"}\n",
    )
    .expect("conflicting state fixture");
    assert!(!development_build(conflict_fixture.path()).success());
    assert!(
        !conflict_fixture.path().join("public").exists(),
        "sequence conflict must precede every public mutation"
    );

    for relative in [
        "marketplace/targets.json",
        "examples/hello-widget/manifest.json",
        "examples/hello-widget/listing.json",
        "examples/hello-widget/component.wasm",
    ] {
        let path = fixture.path().join(relative);
        fs::set_permissions(&path, Permissions::from_mode(0o666)).expect("unsafe source mode");
        assert!(!policy(fixture.path()), "group-writable source {relative}");
        fs::set_permissions(path, Permissions::from_mode(0o644)).expect("restore source mode");
    }

    assert!(inspect(&component));
    let component_link = fixture.path().join("component-link.wasm");
    symlink(&component, &component_link).expect("component symlink");
    assert!(
        !inspect(&component_link),
        "component inspection must use a bounded no-follow read"
    );
    assert!(policy(fixture.path()));
    assert!(!fixture.path().join("public").exists());
    assert!(
        !fixture
            .path()
            .join("marketplace/development-catalog-state.json")
            .exists(),
        "policy must not advance publisher authority"
    );

    let manifest_path = fixture.path().join("examples/hello-widget/manifest.json");
    let manifest_bytes = fs::read(&manifest_path).expect("manifest fixture");
    add_large_png_assets(fixture.path(), &manifest_bytes);
    assert!(
        !policy(fixture.path()),
        "three compressible 2048-square RGBA assets exceed the 32 MiB decoded budget"
    );
    fs::write(&manifest_path, &manifest_bytes).expect("restore manifest fixture");

    let build = || development_build(fixture.path());
    assert!(build().success(), "first build");
    assert_eq!(
        fs::read(
            fixture
                .path()
                .join("marketplace/development-catalog-state.json")
        )
        .expect("development state"),
        b"{\"schemaVersion\":1,\"sequence\":1,\"payloadSha256\":\"2ab96a4bf6bb053c7864b01b00448a34182c87bd2f0a8b76c4dea06601a6c9f9\"}\n"
    );
    let catalog_path = fixture.path().join("public/marketplace/v1/catalog.json");
    let first = fs::read(&catalog_path).expect("first catalog");

    let packages_before = published_package_count(fixture.path());
    let mut changed: Value = serde_json::from_slice(&manifest_bytes).expect("manifest JSON");
    changed["display"]["width"] = 321.into();
    let mut changed = serde_json::to_vec_pretty(&changed).expect("changed manifest");
    changed.push(b'\n');
    fs::write(&manifest_path, changed).expect("changed manifest fixture");
    let conflict = build();
    let packages_after = published_package_count(fixture.path());
    fs::write(&manifest_path, &manifest_bytes).expect("restore manifest fixture");
    assert!(!conflict.success(), "same sequence with changed payload");
    assert_eq!(
        packages_after, packages_before,
        "sequence rejection must happen before content objects are published"
    );

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
    fs::set_permissions(secrets.path(), Permissions::from_mode(0o700))
        .expect("private secrets directory");
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

fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
}

fn inspect(component: &Path) -> bool {
    tool()
        .arg("inspect-component")
        .arg(component)
        .status()
        .expect("inspect component")
        .success()
}

fn policy(repository: &Path) -> bool {
    tool()
        .args([
            "policy",
            "--repository",
            repository.to_str().expect("UTF-8 fixture path"),
        ])
        .status()
        .expect("run package policy")
        .success()
}

fn development_build(repository: &Path) -> std::process::ExitStatus {
    tool()
        .args([
            "build",
            "--repository",
            repository.to_str().expect("UTF-8 fixture path"),
            "--generated-at",
            FIXED_GENERATED,
            "--expires-at",
            FIXED_EXPIRES,
            "--development-key",
        ])
        .status()
        .expect("run marketplace build")
}

fn add_large_png_assets(repository: &Path, original: &[u8]) {
    let mut png = Vec::new();
    let pixels = vec![0; 2_048 * 2_048 * 4];
    PngEncoder::new(&mut png)
        .write_image(&pixels, 2_048, 2_048, ExtendedColorType::Rgba8)
        .expect("compressible PNG fixture");
    assert!(png.len() < 2 * 1024 * 1024, "compressed asset bound");
    let digest = lower_hex(ring::digest::digest(&ring::digest::SHA256, &png).as_ref());
    let source = repository.join("examples/hello-widget");
    fs::create_dir(source.join("assets")).expect("asset fixture directory");
    let mut manifest: Value = serde_json::from_slice(original).expect("manifest JSON");
    for index in 0..3 {
        let relative = format!("assets/large-{index}.png");
        fs::write(source.join(&relative), &png).expect("large asset fixture");
        manifest["files"]["assets"][format!("large-{index}")] = serde_json::json!({
            "path": relative,
            "sha256": digest,
        });
    }
    let mut encoded = serde_json::to_vec_pretty(&manifest).expect("large manifest");
    encoded.push(b'\n');
    fs::write(source.join("manifest.json"), encoded).expect("large manifest fixture");
}

fn published_package_count(repository: &Path) -> usize {
    fs::read_dir(
        repository
            .join("public/marketplace/v1/packages/com.playervox.overcrow.example.hello/0.1.0"),
    )
    .expect("published package directory")
    .count()
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
    let development_key = "fixtures/keys/development-ed25519.key";
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(development_key),
        repository.join(development_key),
    )
    .expect("publisher fixture");
    fs::write(
        repository.join("marketplace/targets.json"),
        b"[{\"sourceDirectory\":\"examples/hello-widget\",\"status\":\"verified\"}]\n",
    )
    .expect("hello-only target fixture");
    fs::write(
        repository.join("marketplace/development-sequence.txt"),
        b"1\n",
    )
    .expect("fixture development sequence");
}
