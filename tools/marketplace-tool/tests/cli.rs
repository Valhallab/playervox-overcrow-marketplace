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
