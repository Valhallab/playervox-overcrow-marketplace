use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use serde_json::Value;

pub(crate) fn repository_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("canonical marketplace repository")
    })
}

pub(crate) fn generated_public_root() -> PathBuf {
    std::env::var_os("OVERCROW_MARKETPLACE_TEST_PUBLIC")
        .map(PathBuf::from)
        .unwrap_or_else(|| repository_root().join("public"))
}

pub(crate) fn generated_catalog_fixture() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        if std::env::var_os("OVERCROW_MARKETPLACE_TEST_PUBLIC").is_none() {
            let status = Command::new("sh")
                .arg("scripts/build-local.sh")
                .current_dir(repository_root())
                .status()
                .expect("start local marketplace generation");
            assert!(
                status.success(),
                "local marketplace generation must succeed"
            );
        }

        let catalog = std::fs::read(generated_public_root().join("marketplace/v1/catalog.json"))
            .expect("generated catalog");
        serde_json::from_slice(&catalog).expect("catalog envelope JSON")
    })
}

fn payload(catalog: &Value) -> Value {
    let encoded = catalog["payload"].as_str().expect("catalog payload");
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
            .expect("base64 catalog payload");
    serde_json::from_slice(&decoded).expect("catalog payload JSON")
}

#[test]
fn generated_catalog_has_five_visible_packages_and_one_hidden_provider() {
    let catalog = payload(generated_catalog_fixture());
    let targets = catalog["targets"].as_array().expect("catalog targets");
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["manifest"]["kind"] != "provider")
            .count(),
        5,
        "widgets and bundles are visible marketplace packages"
    );
    assert_eq!(
        targets
            .iter()
            .filter(|target| target["manifest"]["kind"] == "provider")
            .count(),
        1
    );
    assert!(targets.iter().all(|target| {
        target["manifest"]["availableLocales"] == serde_json::json!(["en", "fr"])
            && target["listing"]["localizations"]
                .as_array()
                .expect("listing localizations")
                .iter()
                .all(|text| matches!(text["locale"].as_str(), Some("en" | "fr")))
    }));
}

#[test]
fn generated_catalog_binds_only_the_four_worldstate_consumers() {
    let catalog = payload(generated_catalog_fixture());
    let targets = catalog["targets"].as_array().expect("catalog targets");
    let provider = targets
        .iter()
        .find(|target| target["manifest"]["id"] == "com.playervox.overcrow.warframe.worldstate")
        .expect("worldstate provider");
    for id in [
        "com.playervox.overcrow.warframe.status",
        "com.playervox.overcrow.warframe.fissures",
        "com.playervox.overcrow.warframe.sortie-archon",
        "com.playervox.overcrow.warframe.invasions",
    ] {
        let target = targets
            .iter()
            .find(|target| target["manifest"]["id"] == id)
            .expect("consumer");
        assert_eq!(
            target["manifest"]["dependencies"],
            serde_json::json!([{
                "id": provider["manifest"]["id"], "version": provider["manifest"]["version"], "sha256": provider["packageSha256"],
            }])
        );
    }
    let market = targets
        .iter()
        .find(|target| target["manifest"]["id"] == "com.playervox.overcrow.warframe.market")
        .expect("market");
    assert_eq!(market["manifest"]["dependencies"], serde_json::json!([]));
}
