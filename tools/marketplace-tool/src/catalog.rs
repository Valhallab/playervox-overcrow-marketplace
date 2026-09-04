use std::{error::Error, fmt, fs, path::Path};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use ring::signature::{Ed25519KeyPair, KeyPair as _};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::{admission, package, private_fs};

const SCHEMA_VERSION: u32 = 1;
const DEVELOPMENT_KEY_ID: &str = "overcrow-development-2026";
const DEVELOPMENT_BASE_URL: &str = "http://127.0.0.1:8787/marketplace/v1/";
const MAX_PAYLOAD_BYTES: usize = 700 * 1024;
const MAX_ENVELOPE_BYTES: usize = 1024 * 1024;
const MAX_SIGNING_KEY_BYTES: u64 = 65;
const MAX_CATALOG_LIFETIME_DAYS: i64 = 90;
const DEVELOPMENT_PUBLIC_KEY: &str = include_str!("../../../fixtures/keys/development-ed25519.pub");

pub struct CatalogStageOptions<'a> {
    pub store: &'a Path,
    pub review_tree: &'a str,
    pub output: &'a Path,
    pub sequence: u64,
    pub generated_at: &'a str,
    pub expires_at: &'a str,
    pub signing_key: &'a Path,
}

pub struct StagedCatalog {
    pub target_count: usize,
}

#[derive(Debug)]
pub struct CatalogStageError;

impl fmt::Display for CatalogStageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("development catalog staging rejected")
    }
}

impl Error for CatalogStageError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPayload<'a> {
    schema_version: u32,
    sequence: u64,
    generated_at: &'a str,
    expires_at: &'a str,
    targets: Vec<CatalogTarget<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogTarget<'a> {
    manifest: &'a serde_json::Value,
    listing: &'a package::Listing,
    package_url: String,
    package_size: u64,
    package_sha256: &'a str,
    status: &'static str,
    preview: Option<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEnvelope<'a> {
    schema_version: u32,
    key_id: &'static str,
    payload: String,
    signature: &'a str,
}

pub fn stage_development(
    options: &CatalogStageOptions<'_>,
) -> Result<StagedCatalog, CatalogStageError> {
    private_fs::validate_private_directory(options.output).map_err(|_| CatalogStageError)?;
    if options.sequence == 0
        || fs::read_dir(options.output)
            .map_err(|_| CatalogStageError)?
            .next()
            .is_some()
    {
        return Err(CatalogStageError);
    }
    let generated_at = parse_timestamp(options.generated_at)?;
    let expires_at = parse_timestamp(options.expires_at)?;
    let maximum_lifetime =
        TimeDelta::try_days(MAX_CATALOG_LIFETIME_DAYS).ok_or(CatalogStageError)?;
    let lifetime = expires_at.signed_duration_since(generated_at);
    if lifetime <= TimeDelta::zero() || lifetime > maximum_lifetime {
        return Err(CatalogStageError);
    }

    let admission = admission::load_verified(options.store, options.review_tree)
        .map_err(|_| CatalogStageError)?;
    let targets = admission
        .artifacts
        .iter()
        .map(|artifact| CatalogTarget {
            manifest: &artifact.manifest,
            listing: &artifact.listing,
            package_url: format!(
                "{DEVELOPMENT_BASE_URL}packages/{}/{}/{}.ocpkg",
                artifact.id, artifact.version, artifact.package_sha256
            ),
            package_size: artifact.package_size,
            package_sha256: &artifact.package_sha256,
            status: "verified",
            preview: None,
        })
        .collect();
    let payload = serde_json::to_vec(&CatalogPayload {
        schema_version: SCHEMA_VERSION,
        sequence: options.sequence,
        generated_at: options.generated_at,
        expires_at: options.expires_at,
        targets,
    })
    .map_err(|_| CatalogStageError)?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(CatalogStageError);
    }

    let seed_bytes = private_fs::read_regular_file(options.signing_key, MAX_SIGNING_KEY_BYTES)
        .map_err(|_| CatalogStageError)?;
    let seed = decode_hex_32(&seed_bytes).ok_or(CatalogStageError)?;
    let expected_public_key =
        decode_hex_32(DEVELOPMENT_PUBLIC_KEY.as_bytes()).ok_or(CatalogStageError)?;
    let key = Ed25519KeyPair::from_seed_unchecked(&seed).map_err(|_| CatalogStageError)?;
    if key.public_key().as_ref() != expected_public_key {
        return Err(CatalogStageError);
    }
    let signature = URL_SAFE_NO_PAD.encode(key.sign(&payload).as_ref());
    let envelope = serde_json::to_vec(&CatalogEnvelope {
        schema_version: SCHEMA_VERSION,
        key_id: DEVELOPMENT_KEY_ID,
        payload: URL_SAFE_NO_PAD.encode(&payload),
        signature: &signature,
    })
    .map_err(|_| CatalogStageError)?;
    if envelope.len() > MAX_ENVELOPE_BYTES {
        return Err(CatalogStageError);
    }

    let packages = private_fs::ensure_private_directory(&options.output.join("packages"))
        .map_err(|_| CatalogStageError)?;
    for artifact in &admission.artifacts {
        let bytes = private_fs::read_regular_file(&artifact.package_path, artifact.package_size)
            .map_err(|_| CatalogStageError)?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if package::sha256_hex(&digest) != artifact.package_sha256 {
            return Err(CatalogStageError);
        }
        let identity = private_fs::ensure_private_directory(&packages.join(&artifact.id))
            .map_err(|_| CatalogStageError)?;
        let version = private_fs::ensure_private_directory(&identity.join(&artifact.version))
            .map_err(|_| CatalogStageError)?;
        let destination = version.join(format!("{}.ocpkg", artifact.package_sha256));
        private_fs::commit_file(options.output, &destination, &bytes)
            .map_err(|_| CatalogStageError)?;
    }
    private_fs::commit_file(
        options.output,
        &options.output.join("catalog.json"),
        &envelope,
    )
    .map_err(|_| CatalogStageError)?;

    Ok(StagedCatalog {
        target_count: admission.artifacts.len(),
    })
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, CatalogStageError> {
    if value.len() != 20 {
        return Err(CatalogStageError);
    }
    let timestamp = DateTime::parse_from_rfc3339(value)
        .map_err(|_| CatalogStageError)?
        .with_timezone(&Utc);
    if timestamp.to_rfc3339_opts(SecondsFormat::Secs, true) != value {
        return Err(CatalogStageError);
    }
    Ok(timestamp)
}

fn decode_hex_32(bytes: &[u8]) -> Option<[u8; 32]> {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return None;
    }
    let mut decoded = [0; 32];
    for (destination, pair) in decoded.iter_mut().zip(bytes.as_chunks::<2>().0) {
        let nibble = |byte| match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => 0,
        };
        *destination = (nibble(pair[0]) << 4) | nibble(pair[1]);
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _, path::Path};

    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use ring::signature::{ED25519, UnparsedPublicKey};
    use serde_json::Value;
    use sha2::{Digest as _, Sha256};

    use super::{CatalogStageOptions, DEVELOPMENT_PUBLIC_KEY, decode_hex_32, stage_development};
    use crate::{admission, package};

    const TRUST_SHA: &str = "1111111111111111111111111111111111111111";
    const REVIEW_SHA: &str = "2222222222222222222222222222222222222222";
    const REVIEW_TREE: &str = "3333333333333333333333333333333333333333";

    #[test]
    fn stages_exact_admission_only_with_compiled_development_key() {
        let scratch = private_tempdir();
        let artifacts = private_directory(scratch.path(), "artifacts");
        let store = private_directory(scratch.path(), "accepted");
        let output = private_directory(scratch.path(), "staged");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello-web");
        let package_path = artifacts.join("1.ocpkg");
        let written = package::write_package(&fixture, &package_path).expect("fixture package");
        let package_bytes = fs::read(&package_path).expect("fixture package bytes");
        let package_sha256 = package::sha256_hex(&written.digest);
        let listing_bytes = fs::read(fixture.join("listing.json")).expect("fixture listing");
        let listing_sha256 = sha256_hex(&listing_bytes);
        fs::write(artifacts.join("1.listing.json"), &listing_bytes)
            .expect("admission listing sidecar");
        let receipt = scratch.path().join("admission.tsv");
        fs::write(
            &receipt,
            format!(
                "admission\t2\t{TRUST_SHA}\t{REVIEW_SHA}\t{REVIEW_TREE}\n\
                 artifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t1.0.0\t{package_sha256}\t{}\t{listing_sha256}\t{}\n",
                package_bytes.len(),
                listing_bytes.len()
            ),
        )
        .expect("admission receipt");
        admission::ingest(
            &receipt,
            &artifacts,
            &store,
            &admission::ExpectedAdmission {
                trust_sha: TRUST_SHA,
                review_sha: REVIEW_SHA,
                review_tree: REVIEW_TREE,
            },
        )
        .expect("fixture admission");

        let staged = stage_development(&CatalogStageOptions {
            store: &store,
            review_tree: REVIEW_TREE,
            output: &output,
            sequence: 1,
            generated_at: "2026-09-04T12:00:00Z",
            expires_at: "2026-10-04T12:00:00Z",
            signing_key: &fixture_key_path(),
        })
        .expect("signed development catalog");

        assert_eq!(staged.target_count, 1);
        let envelope: Value = serde_json::from_slice(
            &fs::read(output.join("catalog.json")).expect("catalog envelope"),
        )
        .expect("catalog JSON");
        assert_eq!(envelope["schemaVersion"], 1);
        assert_eq!(envelope["keyId"], "overcrow-development-2026");
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(envelope["payload"].as_str().expect("catalog payload"))
            .expect("base64url payload");
        let signature = URL_SAFE_NO_PAD
            .decode(envelope["signature"].as_str().expect("catalog signature"))
            .expect("base64url signature");
        let public_key = decode_hex_32(DEVELOPMENT_PUBLIC_KEY.as_bytes())
            .expect("development fixture public key");
        UnparsedPublicKey::new(&ED25519, public_key)
            .verify(&payload_bytes, &signature)
            .expect("signature over exact payload");
        let payload: Value = serde_json::from_slice(&payload_bytes).expect("catalog payload JSON");
        assert_eq!(payload["schemaVersion"], 1);
        assert_eq!(payload["sequence"], 1);
        assert_eq!(payload["generatedAt"], "2026-09-04T12:00:00Z");
        assert_eq!(payload["expiresAt"], "2026-10-04T12:00:00Z");
        assert_eq!(
            payload["targets"][0]["manifest"]["id"],
            "com.playervox.overcrow.hello"
        );
        assert_eq!(
            payload["targets"][0]["listing"],
            serde_json::from_slice::<Value>(&listing_bytes).expect("listing JSON")
        );
        assert_eq!(payload["targets"][0]["packageSha256"], package_sha256);
        assert_eq!(payload["targets"][0]["packageSize"], package_bytes.len());
        assert_eq!(payload["targets"][0]["status"], "verified");
        assert_eq!(payload["targets"][0]["preview"], Value::Null);
        assert_eq!(
            payload["targets"][0]["packageUrl"],
            format!(
                "http://127.0.0.1:8787/marketplace/v1/packages/com.playervox.overcrow.hello/1.0.0/{package_sha256}.ocpkg"
            )
        );
        assert_eq!(
            fs::read(
                output
                    .join("packages/com.playervox.overcrow.hello/1.0.0")
                    .join(format!("{package_sha256}.ocpkg"))
            )
            .expect("staged package"),
            package_bytes
        );

        let rejected_output = private_directory(scratch.path(), "rejected");
        let wrong_key = scratch.path().join("wrong-development.key");
        fs::write(&wrong_key, format!("{}\n", "00".repeat(32))).expect("wrong fixture key");
        fs::set_permissions(&wrong_key, fs::Permissions::from_mode(0o600))
            .expect("wrong fixture key mode");
        assert!(
            stage_development(&CatalogStageOptions {
                store: &store,
                review_tree: REVIEW_TREE,
                output: &rejected_output,
                sequence: 1,
                generated_at: "2026-09-04T12:00:00Z",
                expires_at: "2026-10-04T12:00:00Z",
                signing_key: &wrong_key,
            })
            .is_err()
        );
        assert!(
            fs::read_dir(rejected_output)
                .expect("rejected output directory")
                .next()
                .is_none()
        );
    }

    fn fixture_key_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/keys/development-ed25519.key")
    }

    fn private_tempdir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("private fixture directory");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("private fixture mode");
        directory
    }

    fn private_directory(parent: &Path, name: &str) -> std::path::PathBuf {
        let directory = parent.join(name);
        fs::create_dir(&directory).expect("private subdirectory");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .expect("private subdirectory mode");
        directory
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
