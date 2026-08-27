use crate::catalog::repository_root;
use std::process::Command;

#[test]
fn marketplace_site_is_localized_dom_safe_and_hides_providers() {
    let site = repository_root().join("site");
    let index = std::fs::read_to_string(site.join("index.html")).expect("site index");
    let app = std::fs::read_to_string(site.join("app.js")).expect("site application");
    let styles = std::fs::read_to_string(site.join("styles.css")).expect("site styles");

    assert!(
        index.contains("Content-Security-Policy"),
        "site must declare a CSP"
    );
    assert!(!index.contains("http://") && !index.contains("https://"));
    assert!(index.contains("English") && index.contains("Français"));
    assert!(
        app.contains("textContent"),
        "creator text must use DOM text nodes"
    );
    assert!(
        !app.contains("innerHTML"),
        "site must not parse creator content as HTML"
    );
    assert!(!styles.is_empty());

    let status = Command::new("node")
        .args(["--test", "tests/site-runtime.test.js"])
        .current_dir(repository_root())
        .status()
        .expect("start site runtime tests");
    assert!(status.success(), "site runtime tests must pass");
}
