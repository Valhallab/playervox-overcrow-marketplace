use std::process::Command;

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
