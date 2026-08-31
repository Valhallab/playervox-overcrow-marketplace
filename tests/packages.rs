use crate::catalog::{generated_catalog_fixture, generated_public_root};

#[test]
fn generated_packages_match_catalog_digests_and_keep_provider_hidden() {
    let encoded = generated_catalog_fixture()["payload"]
        .as_str()
        .expect("catalog payload");
    let payload =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
            .expect("base64 catalog payload");
    let catalog: serde_json::Value =
        serde_json::from_slice(&payload).expect("catalog payload JSON");
    let targets = catalog["targets"].as_array().expect("catalog targets");
    assert_eq!(targets.len(), 6);

    for target in targets {
        let package_url = target["packageUrl"].as_str().expect("package URL");
        assert!(package_url.starts_with("http://127.0.0.1:8787/marketplace/v1/packages/"));
        let relative = package_url
            .strip_prefix("http://127.0.0.1:8787/")
            .expect("loopback package URL");
        let package = std::fs::read(generated_public_root().join(relative))
            .expect("published package object");
        assert_eq!(
            ring::digest::digest(&ring::digest::SHA256, &package)
                .as_ref()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            target["packageSha256"].as_str().expect("package digest")
        );
    }

    assert_eq!(
        targets
            .iter()
            .filter(|target| target["manifest"]["kind"] == "provider")
            .count(),
        1
    );
}
