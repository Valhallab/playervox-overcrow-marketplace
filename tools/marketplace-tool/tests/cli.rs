use std::process::Command;

#[test]
fn inspect_rejects_a_fifo_without_waiting_for_a_writer() {
    let scratch = tempfile::tempdir().unwrap();
    let fifo = scratch.path().join("package.ocpkg");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let output = Command::new("timeout")
        .args(["2s", env!("CARGO_BIN_EXE_marketplace-tool"), "inspect"])
        .arg(&fifo)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "FIFO must be rejected before the timeout"
    );
}

fn marketplace_tool(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_marketplace-tool"))
        .args(arguments)
        .output()
        .expect("marketplace-tool should start")
}

#[test]
fn package_rejects_missing_arguments_without_panicking() {
    let output = marketplace_tool(&["package"]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
        "error: invalid package arguments\n"
    );
}

#[test]
fn inspect_rejects_extra_arguments_before_reading_the_package() {
    let output = marketplace_tool(&["inspect", "missing.ocpkg", "unexpected"]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
        "error: invalid inspection arguments\n"
    );
}

#[test]
fn stage_development_catalog_rejects_missing_arguments_without_panicking() {
    let output = marketplace_tool(&["stage-development-catalog"]);

    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
        "error: invalid development catalog staging arguments\n"
    );
}

#[test]
fn playervox_packages_include_their_mit_license() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for widget in ["fixtures/hello-web", "widgets/warframe-market"] {
        let source = root.join(widget);
        let listing: serde_json::Value = serde_json::from_slice(
            &std::fs::read(source.join("listing.json")).expect("listing should exist"),
        )
        .expect("listing should be valid JSON");
        assert_eq!(listing["spdxLicense"], "MIT", "{widget}");
        let output = tempfile::tempdir().expect("private output directory should exist");
        let archive = output.path().join("widget.ocpkg");
        let result = marketplace_tool(&[
            "package",
            source.to_str().expect("source path should be UTF-8"),
            archive.to_str().expect("archive path should be UTF-8"),
        ]);
        assert!(result.status.success(), "{widget}: {:?}", result.stderr);
        let result = marketplace_tool(&[
            "inspect",
            archive.to_str().expect("archive path should be UTF-8"),
        ]);
        assert!(result.status.success(), "{widget}: {:?}", result.stderr);
        let bytes = std::fs::read(archive).expect("archive should exist");
        let notice = std::fs::read(source.join("LICENSE")).expect("MIT notice should exist");
        assert!(notice.starts_with(b"MIT License\n"));
        // Packages are stored ZIPs: the complete grant must travel with the widget.
        assert!(bytes.windows(notice.len()).any(|window| window == notice));
    }
}
