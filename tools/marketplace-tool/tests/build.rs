use std::{
    fs::{self, Permissions},
    os::unix::fs::{PermissionsExt as _, symlink},
    path::Path,
    process::{Command, Output},
};

use image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::Value;

const FIXED_GENERATED: &str = "2026-08-25T00:00:00Z";
const FIXED_EXPIRES: &str = "2036-08-25T00:00:00Z";

#[test]
fn snapshot_plan_accepts_only_a_bounded_regular_reviewed_tree() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    initialize_git_fixture(fixture.path());
    fs::write(fixture.path().join("alpha.txt"), b"alpha\n").expect("regular file");
    fs::write(fixture.path().join("build.sh"), b"#!/bin/sh\nexit 0\n").expect("script");
    fs::set_permissions(
        fixture.path().join("build.sh"),
        Permissions::from_mode(0o755),
    )
    .expect("script mode");
    let revision = commit_git_fixture(fixture.path(), "regular tree");
    let output = snapshot_plan(fixture.path(), &revision);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"2\t23\n");
    assert!(output.stderr.is_empty());

    let fixture = tempfile::tempdir().expect("symlink repository");
    initialize_git_fixture(fixture.path());
    fs::write(fixture.path().join("target"), b"target\n").expect("symlink target");
    symlink("target", fixture.path().join("link")).expect("tracked symlink");
    let revision = commit_git_fixture(fixture.path(), "symlink tree");
    assert!(!snapshot_plan(fixture.path(), &revision).status.success());

    let fixture = tempfile::tempdir().expect("malformed path repository");
    initialize_git_fixture(fixture.path());
    fs::write(fixture.path().join("bad\nname"), b"bad\n").expect("malformed path");
    let revision = commit_git_fixture(fixture.path(), "malformed path");
    assert!(!snapshot_plan(fixture.path(), &revision).status.success());

    let fixture = tempfile::tempdir().expect("entry limit repository");
    initialize_git_fixture(fixture.path());
    fs::create_dir(fixture.path().join("entries")).expect("entry directory");
    for index in 0..1_001 {
        fs::write(fixture.path().join(format!("entries/{index:04}")), b"x").expect("bounded entry");
    }
    let revision = commit_git_fixture(fixture.path(), "excessive entries");
    assert!(!snapshot_plan(fixture.path(), &revision).status.success());

    let fixture = tempfile::tempdir().expect("file limit repository");
    initialize_git_fixture(fixture.path());
    fs::write(
        fixture.path().join("oversized"),
        vec![b'x'; 8 * 1024 * 1024 + 1],
    )
    .expect("oversized file");
    let revision = commit_git_fixture(fixture.path(), "oversized file");
    assert!(!snapshot_plan(fixture.path(), &revision).status.success());

    let fixture = tempfile::tempdir().expect("aggregate limit repository");
    initialize_git_fixture(fixture.path());
    for index in 0..3 {
        fs::write(
            fixture.path().join(format!("aggregate-{index}")),
            vec![b'a' + index; 6 * 1024 * 1024],
        )
        .expect("aggregate file");
    }
    let revision = commit_git_fixture(fixture.path(), "excessive aggregate");
    assert!(!snapshot_plan(fixture.path(), &revision).status.success());
}

#[test]
fn build_plan_emits_only_validated_fixed_fields() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());

    let output = build_plan(fixture.path());
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("UTF-8 plan"),
        "hello-widget\thello_widget\texamples/hello-widget\n",
    );
    assert!(output.stderr.is_empty(), "successful plan stays quiet");
    assert!(
        !fixture.path().join("build-script-ran").exists(),
        "metadata discovery must not execute creator code"
    );
}

#[test]
fn build_plan_rejects_invalid_and_excessive_target_fields() {
    for targets in [
        serde_json::json!([target(
            " examples/hello-widget",
            "hello-widget",
            "hello_widget"
        )]),
        serde_json::json!([target(
            "/examples/hello-widget",
            "hello-widget",
            "hello_widget"
        )]),
        serde_json::json!([target(
            "examples/../hello-widget",
            "hello-widget",
            "hello_widget"
        )]),
        serde_json::json!([target(
            "examples/hello-widget",
            "hello widget",
            "hello_widget"
        )]),
        serde_json::json!([target(
            "examples/hello-widget",
            "hello-widget",
            "hello-widget"
        )]),
        serde_json::json!([{
            "sourceDirectory": "examples/hello-widget",
            "cargoPackage": "hello-widget",
            "componentArtifact": "hello_widget",
            "status": "verified",
            "command": "creator-controlled"
        }]),
    ] {
        let fixture = tempfile::tempdir().expect("fixture repository");
        prepare_build_plan_fixture(fixture.path());
        write_targets(fixture.path(), targets);
        assert!(!build_plan(fixture.path()).status.success());
    }

    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    let targets: Vec<_> = (0..501)
        .map(|index| {
            target(
                &format!("examples/widget-{index}"),
                &format!("widget-{index}"),
                &format!("widget_{index}"),
            )
        })
        .collect();
    write_targets(fixture.path(), serde_json::Value::Array(targets));
    assert!(!build_plan(fixture.path()).status.success());
}

#[test]
fn build_plan_rejects_duplicate_package_and_artifact_identifiers() {
    for second in [
        target("examples/second", "hello-widget", "second"),
        target("examples/second", "second", "hello_widget"),
    ] {
        let fixture = tempfile::tempdir().expect("fixture repository");
        prepare_build_plan_fixture(fixture.path());
        write_targets(
            fixture.path(),
            serde_json::json!([
                target("examples/hello-widget", "hello-widget", "hello_widget"),
                second
            ]),
        );
        assert!(!build_plan(fixture.path()).status.success());
    }
}

#[test]
fn build_plan_rejects_mismatched_or_unsafe_creator_sources() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    fs::create_dir_all(fixture.path().join("examples/declared")).expect("declared source");
    write_targets(
        fixture.path(),
        serde_json::json!([target("examples/declared", "hello-widget", "hello_widget")]),
    );
    assert!(!build_plan(fixture.path()).status.success());

    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    let outside = tempfile::tempdir().expect("outside source");
    symlink(
        outside.path(),
        fixture.path().join("examples/outside-source"),
    )
    .expect("escaping source link");
    write_targets(
        fixture.path(),
        serde_json::json!([target(
            "examples/outside-source",
            "hello-widget",
            "hello_widget"
        )]),
    );
    assert!(!build_plan(fixture.path()).status.success());

    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    fs::set_permissions(
        fixture.path().join("examples/hello-widget"),
        Permissions::from_mode(0o777),
    )
    .expect("unsafe source permissions");
    assert!(
        !build_plan(fixture.path()).status.success(),
        "group-writable creator source must be rejected"
    );
}

#[test]
fn build_plan_rejects_build_scripts_and_proc_macro_targets_without_running_them() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    fs::write(
        fixture.path().join("examples/hello-widget/Cargo.toml"),
        "[package]\nname = \"hello-widget\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
    )
    .expect("build script manifest");
    fs::write(
        fixture.path().join("examples/hello-widget/build.rs"),
        format!(
            "fn main() {{ std::fs::write({:?}, b\"ran\").unwrap(); }}\n",
            fixture.path().join("build-script-ran")
        ),
    )
    .expect("build script fixture");
    assert!(!build_plan(fixture.path()).status.success());
    assert!(!fixture.path().join("build-script-ran").exists());

    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    fs::write(
        fixture.path().join("examples/hello-widget/Cargo.toml"),
        "[package]\nname = \"hello-widget\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n",
    )
    .expect("proc macro manifest");
    assert!(!build_plan(fixture.path()).status.success());
}

#[test]
fn build_plan_rejects_git_custom_registry_and_unlocked_dependencies() {
    for dependency in [
        "bad = { git = \"https://example.invalid/repository\" }",
        "bad = { version = \"1\", registry = \"private\" }",
        "serde = \"1\"",
    ] {
        let fixture = tempfile::tempdir().expect("fixture repository");
        prepare_build_plan_fixture(fixture.path());
        fs::write(
            fixture.path().join("examples/hello-widget/Cargo.toml"),
            format!(
                "[package]\nname = \"hello-widget\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n{dependency}\n"
            ),
        )
        .expect("dependency manifest");
        assert!(!build_plan(fixture.path()).status.success(), "{dependency}");
    }
}

#[test]
fn bind_build_rewrites_only_validated_digest_fields() {
    let fixture = tempfile::tempdir().expect("fixture repository");
    prepare_build_plan_fixture(fixture.path());
    let source = fixture.path().join("examples/hello-widget");
    for relative in ["manifest.json", "listing.json"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/hello-widget")
                .join(relative),
            source.join(relative),
        )
        .expect("metadata fixture");
    }
    let manifest_path = source.join("manifest.json");
    let before: Value = serde_json::from_slice(&fs::read(&manifest_path).expect("manifest"))
        .expect("manifest JSON");
    let bindings = fixture.path().join("bindings.json");
    fs::write(
        &bindings,
        b"{\"schemaVersion\":1,\"components\":[{\"sourceDirectory\":\"examples/hello-widget\",\"sha256\":\"1111111111111111111111111111111111111111111111111111111111111111\"}],\"providers\":[]}\n",
    )
    .expect("bindings fixture");
    fs::set_permissions(&bindings, Permissions::from_mode(0o600)).expect("private bindings");

    let output = tool()
        .args(["bind-build", "--repository"])
        .arg(fixture.path())
        .arg("--bindings")
        .arg(&bindings)
        .output()
        .expect("bind build");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let mut expected = before;
    expected["files"]["component"]["sha256"] = serde_json::Value::String("1".repeat(64));
    let after: Value = serde_json::from_slice(&fs::read(manifest_path).expect("bound manifest"))
        .expect("bound manifest JSON");
    assert_eq!(after, expected);
}

#[test]
fn bind_build_rejects_missing_duplicate_extra_or_unknown_bindings() {
    for value in [
        serde_json::json!({"schemaVersion": 1, "components": [], "providers": []}),
        serde_json::json!({
            "schemaVersion": 1,
            "components": [
                {"sourceDirectory": "examples/hello-widget", "sha256": "11".repeat(32)},
                {"sourceDirectory": "examples/hello-widget", "sha256": "22".repeat(32)}
            ],
            "providers": []
        }),
        serde_json::json!({
            "schemaVersion": 1,
            "components": [{"sourceDirectory": "examples/other", "sha256": "11".repeat(32)}],
            "providers": []
        }),
        serde_json::json!({
            "schemaVersion": 1,
            "components": [{"sourceDirectory": "examples/hello-widget", "sha256": "11".repeat(32), "path": "/tmp/creator"}],
            "providers": []
        }),
    ] {
        let fixture = tempfile::tempdir().expect("fixture repository");
        prepare_bind_fixture(fixture.path());
        let bindings = fixture.path().join("bindings.json");
        let mut bytes = serde_json::to_vec(&value).expect("bindings JSON");
        bytes.push(b'\n');
        fs::write(&bindings, bytes).expect("bindings fixture");
        fs::set_permissions(&bindings, Permissions::from_mode(0o600)).expect("private bindings");
        assert!(
            !tool()
                .args(["bind-build", "--repository"])
                .arg(fixture.path())
                .arg("--bindings")
                .arg(bindings)
                .status()
                .expect("bind build")
                .success()
        );
    }
}

#[test]
fn bind_build_requires_exact_dependency_bearing_provider_bindings_without_partial_rewrites() {
    let fixture = tempfile::tempdir().expect("provider binding repository");
    prepare_provider_bind_fixture(fixture.path());
    let source = fixture.path().join("examples/hello-widget");
    let manifest_path = source.join("manifest.json");
    let before: Value = serde_json::from_slice(&fs::read(&manifest_path).expect("manifest"))
        .expect("manifest JSON");
    let bindings = fixture.path().join("bindings.json");
    write_private_bindings(
        &bindings,
        serde_json::json!({
            "schemaVersion": 1,
            "components": [{
                "sourceDirectory": "examples/hello-widget",
                "sha256": "11".repeat(32)
            }],
            "providers": [{
                "id": "com.playervox.overcrow.warframe.worldstate",
                "version": "1.0.0",
                "sha256": "22".repeat(32)
            }]
        }),
    );
    assert!(bind_build(fixture.path(), &bindings).status.success());
    let mut expected = before;
    expected["files"]["component"]["sha256"] = Value::String("11".repeat(32));
    expected["dependencies"][0]["sha256"] = Value::String("22".repeat(32));
    let after: Value = serde_json::from_slice(&fs::read(&manifest_path).expect("bound manifest"))
        .expect("bound manifest JSON");
    assert_eq!(after, expected);

    for providers in [
        serde_json::json!([]),
        serde_json::json!([
            {
                "id": "com.playervox.overcrow.warframe.worldstate",
                "version": "1.0.0",
                "sha256": "22".repeat(32)
            },
            {
                "id": "com.playervox.overcrow.extra",
                "version": "1.0.0",
                "sha256": "33".repeat(32)
            }
        ]),
        serde_json::json!([
            {
                "id": "com.playervox.overcrow.warframe.worldstate",
                "version": "1.0.0",
                "sha256": "22".repeat(32)
            },
            {
                "id": "com.playervox.overcrow.warframe.worldstate",
                "version": "1.0.0",
                "sha256": "33".repeat(32)
            }
        ]),
        serde_json::json!([{
            "id": "com.playervox.overcrow.warframe.worldstate",
            "version": "1.0.0",
            "sha256": "22".repeat(32),
            "url": "https://creator.invalid/provider"
        }]),
    ] {
        let fixture = tempfile::tempdir().expect("rejected provider binding repository");
        prepare_provider_bind_fixture(fixture.path());
        let manifest_path = fixture.path().join("examples/hello-widget/manifest.json");
        let before = fs::read(&manifest_path).expect("manifest before rejection");
        let bindings = fixture.path().join("bindings.json");
        write_private_bindings(
            &bindings,
            serde_json::json!({
                "schemaVersion": 1,
                "components": [{
                    "sourceDirectory": "examples/hello-widget",
                    "sha256": "11".repeat(32)
                }],
                "providers": providers
            }),
        );
        assert!(!bind_build(fixture.path(), &bindings).status.success());
        assert_eq!(
            fs::read(&manifest_path).expect("manifest after rejection"),
            before,
            "provider rejection must not rewrite any manifest bytes"
        );
    }
}

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

fn build_plan(repository: &Path) -> Output {
    tool()
        .args(["build-plan", "--repository"])
        .arg(repository)
        .output()
        .expect("run build plan")
}

fn snapshot_plan(repository: &Path, revision: &str) -> Output {
    tool()
        .args(["snapshot-plan", "--repository"])
        .arg(repository)
        .arg("--revision")
        .arg(revision)
        .output()
        .expect("run snapshot plan")
}

fn bind_build(repository: &Path, bindings: &Path) -> Output {
    tool()
        .args(["bind-build", "--repository"])
        .arg(repository)
        .arg("--bindings")
        .arg(bindings)
        .output()
        .expect("bind build")
}

fn write_private_bindings(path: &Path, value: Value) {
    let mut bytes = serde_json::to_vec(&value).expect("bindings JSON");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("bindings fixture");
    fs::set_permissions(path, Permissions::from_mode(0o600)).expect("private bindings");
}

fn initialize_git_fixture(repository: &Path) {
    assert!(
        Command::new("/usr/bin/git")
            .args(["init", "--quiet"])
            .arg(repository)
            .status()
            .expect("initialize Git fixture")
            .success()
    );
    for (key, value) in [
        ("user.name", "Marketplace Tests"),
        ("user.email", "marketplace-tests@invalid.example"),
    ] {
        assert!(
            Command::new("/usr/bin/git")
                .args(["-C"])
                .arg(repository)
                .args(["config", key, value])
                .status()
                .expect("configure Git fixture")
                .success()
        );
    }
}

fn commit_git_fixture(repository: &Path, message: &str) -> String {
    assert!(
        Command::new("/usr/bin/git")
            .args(["-C"])
            .arg(repository)
            .args(["add", "--all"])
            .status()
            .expect("stage Git fixture")
            .success()
    );
    assert!(
        Command::new("/usr/bin/git")
            .args(["-C"])
            .arg(repository)
            .args(["commit", "--quiet", "-m", message])
            .status()
            .expect("commit Git fixture")
            .success()
    );
    let revision = Command::new("/usr/bin/git")
        .args(["-C"])
        .arg(repository)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("resolve Git fixture revision");
    assert!(revision.status.success());
    String::from_utf8(revision.stdout)
        .expect("revision UTF-8")
        .trim()
        .to_owned()
}

fn target(source: &str, package: &str, artifact: &str) -> Value {
    serde_json::json!({
        "sourceDirectory": source,
        "cargoPackage": package,
        "componentArtifact": artifact,
        "status": "verified"
    })
}

fn write_targets(repository: &Path, targets: Value) {
    let mut bytes = serde_json::to_vec_pretty(&targets).expect("target JSON");
    bytes.push(b'\n');
    fs::write(repository.join("marketplace/targets.json"), bytes).expect("target fixture");
}

fn prepare_build_plan_fixture(repository: &Path) {
    let source = repository.join("examples/hello-widget");
    fs::create_dir_all(source.join("src")).expect("source directories");
    fs::create_dir_all(repository.join("marketplace")).expect("marketplace directory");
    fs::write(
        repository.join("Cargo.toml"),
        "[workspace]\nmembers = [\"examples/hello-widget\"]\nresolver = \"3\"\n",
    )
    .expect("workspace manifest");
    fs::write(
        repository.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"hello-widget\"\nversion = \"0.1.0\"\n",
    )
    .expect("workspace lockfile");
    fs::write(
        source.join("Cargo.toml"),
        "[package]\nname = \"hello-widget\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
    )
    .expect("package manifest");
    fs::write(source.join("src/lib.rs"), "pub fn fixture() {}\n").expect("package source");
    write_targets(
        repository,
        serde_json::json!([target(
            "examples/hello-widget",
            "hello-widget",
            "hello_widget"
        )]),
    );
}

fn prepare_bind_fixture(repository: &Path) {
    prepare_build_plan_fixture(repository);
    let source = repository.join("examples/hello-widget");
    for relative in ["manifest.json", "listing.json"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/hello-widget")
                .join(relative),
            source.join(relative),
        )
        .expect("metadata fixture");
    }
}

fn prepare_provider_bind_fixture(repository: &Path) {
    prepare_build_plan_fixture(repository);
    let source = repository.join("examples/hello-widget");
    for relative in ["manifest.json", "listing.json"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../widgets/warframe-status")
                .join(relative),
            source.join(relative),
        )
        .expect("dependency-bearing metadata fixture");
    }
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
    prepare_build_plan_fixture(repository);
    let source = repository.join("examples/hello-widget");
    fs::create_dir_all(source.join("locales")).expect("source directories");
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
        repository.join("marketplace/development-sequence.txt"),
        b"1\n",
    )
    .expect("fixture development sequence");
}
