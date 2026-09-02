use std::{
    fs::{self, Permissions},
    os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink},
    path::Path,
    process::{Command, Output},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use image::{ExtendedColorType, ImageEncoder as _, codecs::png::PngEncoder};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde_json::Value;

const FIXED_GENERATED: &str = "2026-08-25T00:00:00Z";
const FIXED_EXPIRES: &str = "2036-08-25T00:00:00Z";
const PRODUCTION_EXPIRES: &str = "2026-11-23T00:00:00Z";
const PRODUCTION_KEY_ID: &str = "overcrow-production-2026-01";

#[test]
fn rename_noreplace_never_clobbers_transaction_destinations() {
    for destination_relative in [
        "public",
        ".public-previous.fixture",
        ".public-next.fixture/tree",
        ".public-quarantine.fixture/slot.0",
    ] {
        let fixture = tempfile::tempdir().expect("rename fixture");
        let live = fixture.path().join("live");
        let staged = fixture.path().join("staged");
        fs::create_dir(&live).expect("live root");
        fs::create_dir(&staged).expect("staged root");
        fs::set_permissions(&live, Permissions::from_mode(0o700)).expect("live root mode");
        fs::set_permissions(&staged, Permissions::from_mode(0o700)).expect("staged root mode");
        let source = staged.join("public");
        fs::create_dir(&source).expect("source directory");
        fs::write(source.join("owned"), b"owned\n").expect("owned bytes");
        let destination = live.join(destination_relative);
        fs::create_dir_all(destination.parent().expect("destination parent"))
            .expect("destination parents");
        fs::create_dir(&destination).expect("foreign empty destination");
        let source_identity = directory_identity(&source);
        let destination_identity = directory_identity(&destination);

        let output = rename_noreplace(&live, &staged, &source, &destination);
        assert!(!output.status.success(), "existing destination must win");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"marketplace-tool: publication failed\n");
        assert_eq!(directory_identity(&source), source_identity);
        assert_eq!(directory_identity(&destination), destination_identity);
        assert_eq!(
            fs::read(source.join("owned")).expect("owned bytes"),
            b"owned\n"
        );
        assert!(
            fs::read_dir(&destination)
                .expect("foreign destination")
                .next()
                .is_none(),
            "foreign empty destination remains the same empty inode"
        );
    }

    let fixture = tempfile::tempdir().expect("successful rename fixture");
    let live = fixture.path().join("live");
    let staged = fixture.path().join("staged");
    fs::create_dir(&live).expect("live root");
    fs::create_dir(&staged).expect("staged root");
    fs::set_permissions(&live, Permissions::from_mode(0o700)).expect("live root mode");
    fs::set_permissions(&staged, Permissions::from_mode(0o700)).expect("staged root mode");
    let source = staged.join("public");
    let next = live.join(".public-next.fixture");
    let destination = next.join("tree");
    fs::create_dir(&source).expect("source directory");
    fs::write(source.join("owned"), b"owned\n").expect("owned bytes");
    fs::create_dir(&next).expect("next wrapper");
    fs::set_permissions(&next, Permissions::from_mode(0o700)).expect("next wrapper mode");
    let source_identity = directory_identity(&source);
    let output = rename_noreplace(&live, &staged, &source, &destination);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"publication=renamed\n");
    assert!(output.stderr.is_empty());
    assert!(!source.exists());
    assert_eq!(directory_identity(&destination), source_identity);
}

#[test]
fn rename_noreplace_rejects_paths_outside_the_owned_transaction() {
    let fixture = tempfile::tempdir().expect("unsafe rename fixture");
    let live = fixture.path().join("live");
    let staged = fixture.path().join("staged");
    fs::create_dir(&live).expect("live root");
    fs::create_dir(&staged).expect("staged root");
    fs::set_permissions(&live, Permissions::from_mode(0o700)).expect("live root mode");
    fs::set_permissions(&staged, Permissions::from_mode(0o700)).expect("staged root mode");
    let source = staged.join("public");
    fs::create_dir(&source).expect("source directory");
    let outside = fixture.path().join("outside");
    fs::create_dir(&outside).expect("outside directory");

    for output in [
        rename_noreplace(&live, &staged, &source, &outside.join("moved")),
        tool()
            .args([
                "rename-noreplace",
                "--live-root",
                "relative",
                "--staged-root",
            ])
            .arg(&staged)
            .args(["--public-name", "public", "--source"])
            .arg(&source)
            .args(["--destination"])
            .arg(live.join("public"))
            .output()
            .expect("relative-root rename"),
    ] {
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"marketplace-tool: publication failed\n");
    }

    let linked_live = fixture.path().join("linked-live");
    symlink(&live, &linked_live).expect("live-root symlink");
    let output = rename_noreplace(&linked_live, &staged, &source, &live.join("public"));
    assert!(!output.status.success());
    assert_eq!(output.stderr, b"marketplace-tool: publication failed\n");

    fs::set_permissions(&live, Permissions::from_mode(0o777)).expect("unsafe live mode");
    let output = rename_noreplace(&live, &staged, &source, &live.join("public"));
    assert!(!output.status.success());
    assert_eq!(output.stderr, b"marketplace-tool: publication failed\n");
    assert!(source.is_dir());
}

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
    assert_eq!(
        output.stdout,
        b"100644\t6\t4a58007052a65fbc2fc3f910f2855f45a4058e74\talpha.txt\n100755\t17\t039e4d0069c5c26909f86c505b9de66182e6d1f3\tbuild.sh\n"
    );
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
        "hello-widget\thello_widget\texamples/hello-widget\t1\n",
    );
    assert!(output.stderr.is_empty(), "successful plan stays quiet");
    assert!(
        !fixture.path().join("build-script-ran").exists(),
        "metadata discovery must not execute creator code"
    );
}

#[test]
fn build_plan_treats_rustc_wrappers_and_cargo_config_as_inert_data() {
    let fixture = tempfile::tempdir().expect("wrapper fixture repository");
    prepare_build_plan_fixture(fixture.path());
    let marker = fixture.path().join("wrapper-ran");
    let original_workspace = fs::read(fixture.path().join("Cargo.toml")).expect("workspace bytes");
    let wrapper = fixture.path().join("creator-wrapper");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' ran >'{}'\nprintf '%s\\n' overwritten >'{}'\nexit 1\n",
            marker.display(),
            fixture.path().join("Cargo.toml").display()
        ),
    )
    .expect("creator wrapper");
    fs::set_permissions(&wrapper, Permissions::from_mode(0o700)).expect("wrapper mode");
    let output = tool()
        .env("RUSTC", &wrapper)
        .env("RUSTC_WRAPPER", &wrapper)
        .args(["build-plan", "--repository"])
        .arg(fixture.path())
        .output()
        .expect("build plan with inherited wrappers");
    assert!(!marker.exists(), "inherited wrapper must not execute");
    assert_eq!(
        fs::read(fixture.path().join("Cargo.toml")).expect("workspace after wrapper"),
        original_workspace,
        "inherited wrapper must not overwrite the snapshot"
    );
    assert!(output.status.success(), "inherited wrappers are ignored");

    let ambient = tempfile::tempdir().expect("ambient config fixture");
    let repository = ambient.path().join("repository");
    fs::create_dir_all(ambient.path().join(".cargo")).expect("ambient Cargo config directory");
    prepare_build_plan_fixture(&repository);
    let marker = ambient.path().join("ambient-wrapper-ran");
    let wrapper = ambient.path().join("ambient-wrapper");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nprintf '%s\\n' ran >'{}'\nexit 1\n",
            marker.display()
        ),
    )
    .expect("ambient wrapper");
    fs::set_permissions(&wrapper, Permissions::from_mode(0o700)).expect("ambient wrapper mode");
    fs::write(
        ambient.path().join(".cargo/config.toml"),
        format!("[build]\nrustc-wrapper = {:?}\n", wrapper),
    )
    .expect("ambient Cargo config");
    let output = build_plan(&repository);
    assert!(output.status.success(), "ancestor config is ignored");
    assert!(!marker.exists(), "ambient config wrapper must not execute");

    fs::create_dir_all(repository.join(".cargo")).expect("tracked Cargo config directory");
    fs::write(
        repository.join(".cargo/config.toml"),
        format!("[build]\nrustc-wrapper = {:?}\n", wrapper),
    )
    .expect("tracked Cargo config");
    let output = build_plan(&repository);
    assert!(
        !output.status.success(),
        "repository Cargo config is rejected"
    );
    assert!(!marker.exists(), "rejected config wrapper must not execute");
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
fn build_plan_rejects_build_target_and_unsupported_manifest_sections() {
    for section in [
        "[build-dependencies]\n",
        "[target.'cfg(target_arch = \"wasm32\")'.dependencies]\n",
        "[features]\ndefault = []\n",
    ] {
        let fixture = tempfile::tempdir().expect("unsupported manifest fixture");
        prepare_build_plan_fixture(fixture.path());
        let manifest = fixture.path().join("examples/hello-widget/Cargo.toml");
        let mut bytes = fs::read(&manifest).expect("package manifest");
        bytes.extend_from_slice(section.as_bytes());
        fs::write(&manifest, bytes).expect("unsupported package manifest");
        assert!(
            !build_plan(fixture.path()).status.success(),
            "unsupported section must be rejected: {section}"
        );
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
    let catalog_path = fixture.path().join("public/marketplace/v1/catalog.json");
    let first = fs::read(&catalog_path).expect("first catalog");
    let envelope: Value = serde_json::from_slice(&first).expect("catalog envelope");
    let payload = URL_SAFE_NO_PAD
        .decode(envelope["payload"].as_str().expect("catalog payload"))
        .expect("catalog payload encoding");
    let payload_sha256 = lower_hex(ring::digest::digest(&ring::digest::SHA256, &payload).as_ref());
    assert_eq!(
        fs::read(
            fixture
                .path()
                .join("marketplace/development-catalog-state.json"),
        )
        .expect("development state"),
        format!("{{\"schemaVersion\":1,\"sequence\":1,\"payloadSha256\":\"{payload_sha256}\"}}\n")
            .as_bytes()
    );

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
    let public_key = fixture.path().join("keys/overcrow-production-2026-01.pub");
    fs::create_dir_all(public_key.parent().expect("pinned key directory"))
        .expect("pinned key directory");
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
            PRODUCTION_EXPIRES,
            "--production",
            "--sequence-file",
            counter.to_str().expect("counter path"),
            "--sequence-state",
            state.to_str().expect("state path"),
            "--signing-key",
            signing_key.to_str().expect("key path"),
            "--key-id",
            PRODUCTION_KEY_ID,
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
        .arg(PRODUCTION_KEY_ID)
        .status()
        .expect("verify production catalog");
    assert!(verified.success(), "production catalog verification");
}

#[test]
fn production_authority_commands_are_private_atomic_and_payload_bound() {
    let repository = tempfile::tempdir().expect("repository");
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
        repository.path(),
        &target
            .path()
            .join("wasm32-wasip2/release/hello_widget.wasm"),
    );
    fs::create_dir_all(repository.path().join("public/marketplace"))
        .expect("marketplace static directory");
    for directory in ["public", "public/marketplace"] {
        fs::set_permissions(
            repository.path().join(directory),
            Permissions::from_mode(0o755),
        )
        .expect("reviewed static directory mode");
    }
    for (relative, bytes) in [
        ("index.html", b"reviewed static\n".as_slice()),
        ("marketplace/app.js", b"reviewed application\n".as_slice()),
        ("marketplace/styles.css", b"reviewed styles\n".as_slice()),
        (
            "marketplace/catalog-policy.js",
            b"reviewed content policy\n".as_slice(),
        ),
    ] {
        fs::write(repository.path().join("public").join(relative), bytes)
            .expect("reviewed static file");
        fs::set_permissions(
            repository.path().join("public").join(relative),
            Permissions::from_mode(0o644),
        )
        .expect("reviewed static mode");
    }

    let secrets = tempfile::tempdir().expect("external production secrets");
    fs::set_permissions(secrets.path(), Permissions::from_mode(0o700))
        .expect("private secrets directory");
    let sequence = secrets.path().join("sequence.txt");
    let state = secrets.path().join("state.json");
    let signing_key = secrets.path().join("signing.key");
    let public_key = secrets.path().join("signing.pub");
    let seed = [42; 32];
    write_private(&sequence, b"1\n");
    write_private(&signing_key, format!("{}\n", lower_hex(&seed)).as_bytes());

    let derived = tool()
        .args(["derive-public-key", "--repository"])
        .arg(repository.path())
        .arg("--signing-key")
        .arg(&signing_key)
        .arg("--key-id")
        .arg(PRODUCTION_KEY_ID)
        .arg("--output")
        .arg(&public_key)
        .output()
        .expect("derive fixture public key");
    assert!(derived.status.success(), "public derivation succeeds");
    assert_eq!(derived.stdout, b"public-key=derived\n");
    assert!(derived.stderr.is_empty());
    assert_eq!(
        fs::metadata(&public_key)
            .expect("public key output")
            .permissions()
            .mode()
            & 0o7777,
        0o644
    );
    let key_bytes = fs::read(&public_key).expect("public key bytes");
    assert!(
        !derived
            .stdout
            .windows(16)
            .any(|window| key_bytes.windows(16).any(|key| key == window))
    );

    let pinned_key = repository
        .path()
        .join("keys/overcrow-production-2026-01.pub");
    fs::create_dir_all(pinned_key.parent().expect("pinned key directory"))
        .expect("pinned key directory");
    fs::copy(&public_key, &pinned_key).expect("fixture pinned public key");
    fs::set_permissions(&pinned_key, Permissions::from_mode(0o644))
        .expect("pinned public key mode");

    let mismatched_key = secrets.path().join("mismatched.key");
    let mismatched_state = secrets.path().join("mismatched-state.json");
    write_private(
        &mismatched_key,
        format!("{}\n", lower_hex(&[43; 32])).as_bytes(),
    );
    let mismatched = production_build(
        repository.path(),
        &sequence,
        &mismatched_state,
        &mismatched_key,
        FIXED_GENERATED,
        PRODUCTION_EXPIRES,
    );
    assert!(
        !mismatched.status.success(),
        "production signing seed must match the repository-pinned public key"
    );
    assert!(!mismatched_state.exists());

    let development_seed = secrets.path().join("development.key");
    write_private(
        &development_seed,
        b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\n",
    );
    for (key, key_id) in [
        (&development_seed, PRODUCTION_KEY_ID),
        (&signing_key, "overcrow-development-2026"),
    ] {
        let rejected_output = secrets.path().join(format!("rejected-{}", key_id));
        let rejected = tool()
            .args(["derive-public-key", "--repository"])
            .arg(repository.path())
            .arg("--signing-key")
            .arg(key)
            .arg("--key-id")
            .arg(key_id)
            .arg("--output")
            .arg(&rejected_output)
            .output()
            .expect("reject development authority");
        assert!(!rejected.status.success());
        assert!(!rejected_output.exists());
        assert!(
            !String::from_utf8_lossy(&rejected.stderr)
                .contains(secrets.path().to_string_lossy().as_ref())
        );
    }

    let built = production_build(
        repository.path(),
        &sequence,
        &state,
        &signing_key,
        FIXED_GENERATED,
        PRODUCTION_EXPIRES,
    );
    assert!(built.status.success(), "production signer fixture");
    let catalog = repository.path().join("public/marketplace/v1/catalog.json");

    let tampered_catalog = secrets.path().join("tampered-catalog.json");
    let mut tampered: Value = serde_json::from_slice(&fs::read(&catalog).expect("signed catalog"))
        .expect("catalog envelope");
    tampered["signature"] = Value::String("A".repeat(86));
    fs::write(
        &tampered_catalog,
        serde_json::to_vec(&tampered).expect("tampered envelope"),
    )
    .expect("tampered catalog");
    let rejected_signature = tool()
        .args(["advance-sequence", "--repository"])
        .arg(repository.path())
        .arg("--sequence-file")
        .arg(&sequence)
        .arg("--sequence-state")
        .arg(&state)
        .arg("--catalog")
        .arg(&tampered_catalog)
        .output()
        .expect("reject unsigned accepted identity");
    assert!(
        !rejected_signature.status.success(),
        "counter advancement must cryptographically verify the accepted identity"
    );
    assert_eq!(fs::read(&sequence).expect("unchanged counter"), b"1\n");

    fs::write(
        repository.path().join("public/index.html"),
        b"raced static\n",
    )
    .expect("mutate accepted tree");
    let rejected_tree = tool()
        .args(["advance-sequence", "--repository"])
        .arg(repository.path())
        .arg("--sequence-file")
        .arg(&sequence)
        .arg("--sequence-state")
        .arg(&state)
        .arg("--catalog")
        .arg(&catalog)
        .output()
        .expect("reject changed accepted tree");
    assert!(
        !rejected_tree.status.success(),
        "counter advancement must reverify the complete signed tree"
    );
    assert_eq!(fs::read(&sequence).expect("unchanged counter"), b"1\n");
    fs::write(
        repository.path().join("public/index.html"),
        b"reviewed static\n",
    )
    .expect("restore accepted tree");

    let advanced = tool()
        .args(["advance-sequence", "--repository"])
        .arg(repository.path())
        .arg("--sequence-file")
        .arg(&sequence)
        .arg("--sequence-state")
        .arg(&state)
        .arg("--catalog")
        .arg(&catalog)
        .output()
        .expect("advance accepted sequence");
    assert!(advanced.status.success(), "accepted payload advances");
    assert_eq!(advanced.stdout, b"sequence=advanced\n");
    assert!(advanced.stderr.is_empty());
    assert_eq!(fs::read(&sequence).expect("advanced counter"), b"2\n");
    assert_eq!(
        fs::metadata(&sequence)
            .expect("counter metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );

    let before = fs::read(&sequence).expect("counter before rejected retry");
    let rejected = tool()
        .args(["advance-sequence", "--repository"])
        .arg(repository.path())
        .arg("--sequence-file")
        .arg(&sequence)
        .arg("--sequence-state")
        .arg(&state)
        .arg("--catalog")
        .arg(&catalog)
        .output()
        .expect("reject stale accepted payload");
    assert!(!rejected.status.success());
    assert_eq!(fs::read(&sequence).expect("unchanged counter"), before);
    assert!(
        !String::from_utf8_lossy(&rejected.stderr)
            .contains(secrets.path().to_string_lossy().as_ref())
    );
}

#[test]
fn verify_tree_accepts_only_the_exact_public_key_bound_object_tree() {
    let repository = tempfile::tempdir().expect("repository");
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
        repository.path(),
        &target
            .path()
            .join("wasm32-wasip2/release/hello_widget.wasm"),
    );
    fs::create_dir_all(repository.path().join("public")).expect("static tree root");
    fs::write(
        repository.path().join("public/index.html"),
        b"reviewed static\n",
    )
    .expect("reviewed static file");
    fs::set_permissions(
        repository.path().join("public/index.html"),
        Permissions::from_mode(0o644),
    )
    .expect("reviewed static mode");
    let secrets = tempfile::tempdir().expect("external production secrets");
    fs::set_permissions(secrets.path(), Permissions::from_mode(0o700))
        .expect("private secrets directory");
    let sequence = secrets.path().join("sequence.txt");
    let state = secrets.path().join("state.json");
    let signing_key = secrets.path().join("signing.key");
    let public_key = repository
        .path()
        .join("keys/overcrow-production-2026-01.pub");
    fs::create_dir_all(public_key.parent().expect("pinned key directory"))
        .expect("pinned key directory");
    write_private(&sequence, b"1\n");
    write_private(
        &signing_key,
        format!("{}\n", lower_hex(&[42; 32])).as_bytes(),
    );
    let pair = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).expect("fixture key");
    fs::write(
        &public_key,
        format!("{}\n", lower_hex(pair.public_key().as_ref())),
    )
    .expect("public key fixture");
    fs::set_permissions(&public_key, Permissions::from_mode(0o644)).expect("public key mode");
    assert!(
        production_build(
            repository.path(),
            &sequence,
            &state,
            &signing_key,
            FIXED_GENERATED,
            PRODUCTION_EXPIRES,
        )
        .status
        .success()
    );
    let staged_parent = repository
        .path()
        .join(".build-production.fixture/repository");
    fs::create_dir_all(&staged_parent).expect("allowed staging parent");
    let tree = staged_parent.join("public");
    assert!(
        Command::new("/usr/bin/cp")
            .args(["-R", "--"])
            .arg(repository.path().join("public"))
            .arg(&tree)
            .status()
            .expect("copy signed tree")
            .success()
    );

    let verify = |candidate: &Path| {
        tool()
            .args(["verify-tree", "--repository"])
            .arg(repository.path())
            .arg("--tree")
            .arg(candidate)
            .arg("--public-key")
            .arg(&public_key)
            .arg("--key-id")
            .arg(PRODUCTION_KEY_ID)
            .output()
            .expect("verify public tree")
    };
    let accepted = verify(&tree);
    assert!(accepted.status.success(), "exact signed tree accepted");
    assert_eq!(accepted.stdout, b"tree=verified\n");
    assert!(accepted.stderr.is_empty());

    let ledger = secrets.path().join("tree.ledger");
    let written = tool()
        .args(["write-tree-ledger", "--repository"])
        .arg(repository.path())
        .arg("--tree")
        .arg(&tree)
        .arg("--output")
        .arg(&ledger)
        .output()
        .expect("write verified tree ledger");
    assert!(written.status.success(), "verified ledger is written");
    assert_eq!(written.stdout, b"tree-ledger=written\n");
    assert!(written.stderr.is_empty());
    assert_eq!(
        fs::metadata(&ledger)
            .expect("tree ledger metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    let ledger_digest = lower_hex(
        ring::digest::digest(
            &ring::digest::SHA256,
            &fs::read(&ledger).expect("tree ledger"),
        )
        .as_ref(),
    );
    let verify_ledger = |candidate: &Path| {
        tool()
            .args(["verify-tree-ledger", "--tree"])
            .arg(candidate)
            .arg("--ledger")
            .arg(&ledger)
            .arg("--sha256")
            .arg(&ledger_digest)
            .output()
            .expect("verify exact tree ledger")
    };
    assert!(verify_ledger(&tree).status.success());

    fs::write(tree.join("unexpected.js"), b"unexpected\n").expect("extra static file");
    fs::set_permissions(tree.join("unexpected.js"), Permissions::from_mode(0o644))
        .expect("extra static mode");
    assert!(
        !verify(&tree).status.success(),
        "extra non-object static files must be rejected"
    );
    assert!(
        !verify_ledger(&tree).status.success(),
        "post-verification static mutation must violate the final ledger"
    );
    fs::remove_file(tree.join("unexpected.js")).expect("remove extra static file");

    for (relative, original) in [
        ("index.html", b"reviewed static\n".as_slice()),
        ("marketplace/app.js", b"reviewed application\n".as_slice()),
        ("marketplace/styles.css", b"reviewed styles\n".as_slice()),
        (
            "marketplace/catalog-policy.js",
            b"reviewed content policy\n".as_slice(),
        ),
    ] {
        fs::write(tree.join(relative), b"modified static\n")
            .unwrap_or_else(|error| panic!("modify static file {relative}: {error}"));
        assert!(
            !verify(&tree).status.success(),
            "modified static file must be rejected: {relative}"
        );
        fs::write(tree.join(relative), original).expect("restore static file");
    }
    fs::remove_file(tree.join("index.html")).expect("remove static file");
    assert!(
        !verify(&tree).status.success(),
        "missing static files must be rejected"
    );
    fs::write(tree.join("index.html"), b"reviewed static\n").expect("restore static file");
    fs::set_permissions(tree.join("index.html"), Permissions::from_mode(0o644))
        .expect("restore static mode");

    fs::create_dir(tree.join("extra-directory")).expect("extra static directory");
    fs::set_permissions(tree.join("extra-directory"), Permissions::from_mode(0o755))
        .expect("extra directory mode");
    assert!(
        !verify(&tree).status.success(),
        "extra empty directories must be rejected"
    );
    fs::remove_dir(tree.join("extra-directory")).expect("remove extra directory");
    fs::set_permissions(tree.join("marketplace"), Permissions::from_mode(0o700))
        .expect("unsafe static directory mode");
    assert!(
        !verify(&tree).status.success(),
        "unsafe static directory modes must be rejected"
    );
    fs::set_permissions(tree.join("marketplace"), Permissions::from_mode(0o755))
        .expect("restore static directory mode");
    fs::remove_file(tree.join("marketplace/app.js")).expect("remove static JavaScript");
    symlink("../index.html", tree.join("marketplace/app.js")).expect("static symlink");
    assert!(
        !verify(&tree).status.success(),
        "static symlinks must be rejected"
    );
    fs::remove_file(tree.join("marketplace/app.js")).expect("remove static symlink");
    fs::write(tree.join("marketplace/app.js"), b"reviewed application\n")
        .expect("restore static JavaScript");
    fs::set_permissions(
        tree.join("marketplace/app.js"),
        Permissions::from_mode(0o644),
    )
    .expect("restore static JavaScript mode");

    let package = fs::read_dir(
        tree.join("marketplace/v1/packages/com.playervox.overcrow.example.hello/0.1.0"),
    )
    .expect("package directory")
    .next()
    .expect("package object")
    .expect("package entry")
    .path();
    let original = fs::read(&package).expect("package bytes");
    fs::write(&package, b"tampered").expect("tamper package");
    assert!(!verify(&tree).status.success(), "digest mismatch rejected");
    fs::write(&package, &original).expect("restore package");
    fs::set_permissions(&package, Permissions::from_mode(0o664)).expect("unsafe object mode");
    assert!(
        !verify(&tree).status.success(),
        "unsafe object mode rejected"
    );
    fs::set_permissions(&package, Permissions::from_mode(0o644)).expect("restore object mode");
    let extra = package.with_file_name(format!("{}.ocpkg", "f".repeat(64)));
    fs::write(&extra, b"extra").expect("extra package");
    assert!(
        !verify(&tree).status.success(),
        "extra content-addressed object rejected"
    );
    fs::remove_file(extra).expect("remove extra package");

    let preview = fs::read_dir(
        tree.join("marketplace/v1/previews/com.playervox.overcrow.example.hello/0.1.0"),
    )
    .expect("preview directory")
    .next()
    .expect("preview object")
    .expect("preview entry")
    .path();
    let original_preview = fs::read(&preview).expect("preview bytes");
    fs::write(&preview, b"tampered image").expect("tamper preview image");
    assert!(
        !verify(&tree).status.success(),
        "modified preview image must be rejected"
    );
    fs::write(&preview, original_preview).expect("restore preview image");

    let outside = tempfile::tempdir().expect("outside tree");
    assert!(
        !verify(outside.path()).status.success(),
        "outside tree rejected"
    );
    let tree_link = repository
        .path()
        .join(".build-production.fixture/tree-link");
    symlink(&tree, &tree_link).expect("tree symlink");
    assert!(
        !verify(&tree_link).status.success(),
        "symlinked tree rejected"
    );
}

#[test]
fn production_build_requires_the_exact_key_and_ninety_day_window() {
    let repository = tempfile::tempdir().expect("repository");
    let target = tempfile::tempdir().expect("isolated cargo target");
    assert!(
        Command::new(env!("CARGO"))
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
            .expect("build hello fixture")
            .success()
    );
    prepare_fixture(
        repository.path(),
        &target
            .path()
            .join("wasm32-wasip2/release/hello_widget.wasm"),
    );
    let secrets = tempfile::tempdir().expect("production secrets");
    fs::set_permissions(secrets.path(), Permissions::from_mode(0o700)).expect("private directory");
    let sequence = secrets.path().join("sequence.txt");
    let state = secrets.path().join("state.json");
    let signing_key = secrets.path().join("signing.key");
    write_private(&sequence, b"1\n");
    write_private(
        &signing_key,
        format!("{}\n", lower_hex(&[42; 32])).as_bytes(),
    );
    let public_key = repository
        .path()
        .join("keys/overcrow-production-2026-01.pub");
    fs::create_dir_all(public_key.parent().expect("pinned key directory"))
        .expect("pinned key directory");
    let pair = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).expect("fixture key");
    fs::write(
        &public_key,
        format!("{}\n", lower_hex(pair.public_key().as_ref())),
    )
    .expect("pinned public key");
    fs::set_permissions(&public_key, Permissions::from_mode(0o644)).expect("pinned key mode");

    let wrong_expiry = production_build(
        repository.path(),
        &sequence,
        &state,
        &signing_key,
        FIXED_GENERATED,
        "2026-11-22T23:59:59Z",
    );
    assert!(!wrong_expiry.status.success(), "non-90-day expiry rejected");
    assert!(!repository.path().join("public").exists());

    let wrong_key = tool()
        .args(["build", "--repository"])
        .arg(repository.path())
        .args([
            "--generated-at",
            FIXED_GENERATED,
            "--expires-at",
            PRODUCTION_EXPIRES,
            "--production",
            "--sequence-file",
        ])
        .arg(&sequence)
        .arg("--sequence-state")
        .arg(&state)
        .arg("--signing-key")
        .arg(&signing_key)
        .args(["--key-id", "overcrow-production-test"])
        .output()
        .expect("reject wrong production key ID");
    assert!(!wrong_key.status.success());
    assert!(!repository.path().join("public").exists());

    write_private(&sequence, b"9007199254740991\n");
    let browser_maximum = production_build(
        repository.path(),
        &sequence,
        &state,
        &signing_key,
        FIXED_GENERATED,
        PRODUCTION_EXPIRES,
    );
    assert!(
        browser_maximum.status.success(),
        "maximum exact JavaScript sequence must build"
    );
    let verified_maximum = tool()
        .args(["verify-tree", "--repository"])
        .arg(repository.path())
        .arg("--tree")
        .arg(repository.path().join("public"))
        .arg("--public-key")
        .arg(&public_key)
        .args(["--key-id", PRODUCTION_KEY_ID])
        .output()
        .expect("verify maximum-sequence tree");
    assert!(
        verified_maximum.status.success(),
        "maximum exact JavaScript sequence must verify"
    );
    write_private(&sequence, b"9007199254740992\n");
    let browser_unsafe_sequence = production_build(
        repository.path(),
        &sequence,
        &state,
        &signing_key,
        FIXED_GENERATED,
        PRODUCTION_EXPIRES,
    );
    assert!(
        !browser_unsafe_sequence.status.success(),
        "production sequence must remain an exact JavaScript integer"
    );
}

fn tool() -> Command {
    Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
}

fn rename_noreplace(live: &Path, staged: &Path, source: &Path, destination: &Path) -> Output {
    tool()
        .args(["rename-noreplace", "--live-root"])
        .arg(live)
        .args(["--staged-root"])
        .arg(staged)
        .args(["--public-name", "public", "--source"])
        .arg(source)
        .args(["--destination"])
        .arg(destination)
        .output()
        .expect("rename-noreplace command")
}

fn directory_identity(path: &Path) -> (u64, u64) {
    let metadata = fs::symlink_metadata(path).expect("directory metadata");
    assert!(metadata.is_dir(), "identity belongs to a directory");
    (metadata.dev(), metadata.ino())
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
    for relative in ["manifest.json", "listing.json"] {
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../examples/hello-widget")
                .join(relative),
            source.join(relative),
        )
        .expect("metadata fixture");
    }
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

fn production_build(
    repository: &Path,
    sequence: &Path,
    state: &Path,
    signing_key: &Path,
    generated_at: &str,
    expires_at: &str,
) -> Output {
    tool()
        .args(["build", "--repository"])
        .arg(repository)
        .args([
            "--generated-at",
            generated_at,
            "--expires-at",
            expires_at,
            "--production",
            "--sequence-file",
        ])
        .arg(sequence)
        .arg("--sequence-state")
        .arg(state)
        .arg("--signing-key")
        .arg(signing_key)
        .args(["--key-id", PRODUCTION_KEY_ID])
        .output()
        .expect("run production marketplace build")
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
    let manifest_path = source.join("manifest.json");
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("fixture manifest bytes"))
            .expect("fixture manifest JSON");
    manifest["files"]["component"]["sha256"] = Value::String(lower_hex(
        ring::digest::digest(
            &ring::digest::SHA256,
            &fs::read(component).expect("component fixture bytes"),
        )
        .as_ref(),
    ));
    let mut manifest_bytes = serde_json::to_vec_pretty(&manifest).expect("fixture manifest JSON");
    manifest_bytes.push(b'\n');
    fs::write(manifest_path, manifest_bytes).expect("fixture manifest");
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
