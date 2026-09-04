use std::{collections::BTreeSet, fmt, fs, path::Path};

use semver::Version;
use sha2::{Digest as _, Sha256};

use crate::{
    package,
    private_fs::{
        PrivateFsError, commit_file, ensure_private_directory, read_regular_file,
        validate_private_directory,
    },
};

const MAX_RECEIPT_BYTES: usize = 256 * 1024;
const MAX_ARTIFACTS: usize = 512;

#[derive(Clone, Copy)]
pub struct ExpectedAdmission<'a> {
    pub trust_sha: &'a str,
    pub review_sha: &'a str,
    pub review_tree: &'a str,
}

#[derive(Debug)]
pub struct AdmissionError;

impl fmt::Display for AdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("admission ingestion rejected")
    }
}

impl From<PrivateFsError> for AdmissionError {
    fn from(_: PrivateFsError) -> Self {
        Self
    }
}

struct Receipt {
    trust_sha: String,
    review_sha: String,
    review_tree: String,
    artifacts: Vec<Artifact>,
}

struct Artifact {
    id: String,
    version: String,
    digest: String,
    bytes: u64,
    listing_digest: String,
    listing_bytes: u64,
}

#[derive(Debug)]
pub struct StoredAdmission {
    pub review_tree: String,
    pub artifact_count: usize,
}

pub(crate) struct VerifiedAdmission {
    pub(crate) artifacts: Vec<AdmittedArtifact>,
}

pub(crate) struct AdmittedArtifact {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) package_sha256: String,
    pub(crate) package_size: u64,
    pub(crate) package_path: std::path::PathBuf,
    pub(crate) manifest: serde_json::Value,
    pub(crate) listing: package::Listing,
}

fn parse_receipt(
    bytes: &[u8],
    expected: &ExpectedAdmission<'_>,
) -> Result<Receipt, AdmissionError> {
    let receipt = parse_unbound_receipt(bytes)?;
    if receipt.trust_sha != expected.trust_sha
        || receipt.review_sha != expected.review_sha
        || receipt.review_tree != expected.review_tree
    {
        return Err(AdmissionError);
    }
    Ok(receipt)
}

fn parse_unbound_receipt(bytes: &[u8]) -> Result<Receipt, AdmissionError> {
    if bytes.is_empty()
        || bytes.len() > MAX_RECEIPT_BYTES
        || !bytes.ends_with(b"\n")
        || bytes.contains(&b'\r')
    {
        return Err(AdmissionError);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| AdmissionError)?;
    if !text
        .bytes()
        .all(|byte| byte == b'\n' || byte == b'\t' || byte.is_ascii_graphic())
    {
        return Err(AdmissionError);
    }
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or(AdmissionError)?
        .split('\t')
        .collect::<Vec<_>>();
    let ["admission", "2", trust_sha, review_sha, review_tree] = header.as_slice() else {
        return Err(AdmissionError);
    };
    if !valid_object_id(trust_sha) || !valid_object_id(review_sha) || !valid_object_id(review_tree)
    {
        return Err(AdmissionError);
    }

    let mut identities = BTreeSet::new();
    let mut previous_source = None::<String>;
    let mut artifacts = Vec::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        let [
            "artifact",
            source,
            id,
            version,
            digest,
            byte_count,
            listing_digest,
            listing_byte_count,
        ] = fields.as_slice()
        else {
            return Err(AdmissionError);
        };
        let parsed_bytes = byte_count.parse::<u64>().map_err(|_| AdmissionError)?;
        let parsed_listing_bytes = listing_byte_count
            .parse::<u64>()
            .map_err(|_| AdmissionError)?;
        if parsed_bytes == 0
            || parsed_bytes > package::MAX_PACKAGE_BYTES as u64
            || parsed_bytes.to_string() != *byte_count
            || parsed_listing_bytes == 0
            || parsed_listing_bytes > package::MAX_LISTING_BYTES as u64
            || parsed_listing_bytes.to_string() != *listing_byte_count
            || !valid_widget_source(source)
            || !package::valid_extension_id(id)
            || !package::canonical_semver(version)
            || !valid_digest(digest)
            || !valid_digest(listing_digest)
            || !identities.insert(*id)
            || previous_source
                .as_deref()
                .is_some_and(|previous| previous >= *source)
        {
            return Err(AdmissionError);
        }
        previous_source = Some((*source).to_owned());
        artifacts.push(Artifact {
            id: (*id).to_owned(),
            version: (*version).to_owned(),
            digest: (*digest).to_owned(),
            bytes: parsed_bytes,
            listing_digest: (*listing_digest).to_owned(),
            listing_bytes: parsed_listing_bytes,
        });
        if artifacts.len() > MAX_ARTIFACTS {
            return Err(AdmissionError);
        }
    }
    if artifacts.is_empty() {
        return Err(AdmissionError);
    }
    Ok(Receipt {
        trust_sha: (*trust_sha).to_owned(),
        review_sha: (*review_sha).to_owned(),
        review_tree: (*review_tree).to_owned(),
        artifacts,
    })
}

pub fn ingest(
    receipt_path: &Path,
    artifacts_root: &Path,
    store: &Path,
    expected: &ExpectedAdmission<'_>,
) -> Result<StoredAdmission, AdmissionError> {
    validate_private_directory(artifacts_root)?;
    validate_private_directory(store)?;
    let receipt_bytes = read_regular_file(receipt_path, MAX_RECEIPT_BYTES as u64)?;
    let receipt = parse_receipt(&receipt_bytes, expected)?;
    validate_artifact_inventory(artifacts_root, receipt.artifacts.len())?;
    let packages_root = ensure_private_directory(&store.join("packages"))?;
    let listings_root = ensure_private_directory(&store.join("listings"))?;
    let admissions_root = ensure_private_directory(&store.join("admissions"))?;
    let stored_receipt = admissions_root.join(format!("{}.tsv", receipt.review_tree));
    if stored_receipt.exists() {
        if read_regular_file(&stored_receipt, MAX_RECEIPT_BYTES as u64)? != receipt_bytes {
            return Err(AdmissionError);
        }
        return verify(store, &receipt.review_tree);
    }
    validate_version_policy(&admissions_root, &receipt)?;

    for (index, artifact) in receipt.artifacts.iter().enumerate() {
        let source = artifacts_root.join(format!("{}.ocpkg", index + 1));
        let package_bytes = read_regular_file(&source, artifact.bytes)?;
        validate_package_bytes(&package_bytes, artifact)?;
        let listing_source = artifacts_root.join(format!("{}.listing.json", index + 1));
        let listing_bytes = read_regular_file(&listing_source, artifact.listing_bytes)?;
        validate_listing_bytes(&listing_bytes, artifact)?;
        let identity_root = ensure_private_directory(&packages_root.join(&artifact.id))?;
        let version_root = ensure_private_directory(&identity_root.join(&artifact.version))?;
        let destination = version_root.join(format!("{}.ocpkg", artifact.digest));
        commit_file(store, &destination, &package_bytes)?;
        let listing_identity_root = ensure_private_directory(&listings_root.join(&artifact.id))?;
        let listing_version_root =
            ensure_private_directory(&listing_identity_root.join(&artifact.version))?;
        let listing_destination =
            listing_version_root.join(format!("{}.json", artifact.listing_digest));
        commit_file(store, &listing_destination, &listing_bytes)?;
    }
    commit_file(store, &stored_receipt, &receipt_bytes)?;
    verify(store, &receipt.review_tree)
}

pub fn verify(store: &Path, review_tree: &str) -> Result<StoredAdmission, AdmissionError> {
    let admission = load_verified(store, review_tree)?;
    Ok(StoredAdmission {
        review_tree: review_tree.to_owned(),
        artifact_count: admission.artifacts.len(),
    })
}

pub(crate) fn load_verified(
    store: &Path,
    review_tree: &str,
) -> Result<VerifiedAdmission, AdmissionError> {
    validate_private_directory(store)?;
    if !valid_object_id(review_tree) {
        return Err(AdmissionError);
    }
    let receipt_path = store.join("admissions").join(format!("{review_tree}.tsv"));
    let receipt_bytes = read_regular_file(&receipt_path, MAX_RECEIPT_BYTES as u64)?;
    let receipt = parse_unbound_receipt(&receipt_bytes)?;
    if receipt.review_tree != review_tree {
        return Err(AdmissionError);
    }
    let mut artifacts = Vec::with_capacity(receipt.artifacts.len());
    for artifact in receipt.artifacts {
        let package_path = store
            .join("packages")
            .join(&artifact.id)
            .join(&artifact.version)
            .join(format!("{}.ocpkg", artifact.digest));
        let package_bytes = read_regular_file(&package_path, artifact.bytes)?;
        let manifest = validate_package_bytes(&package_bytes, &artifact)?;
        let listing_path = store
            .join("listings")
            .join(&artifact.id)
            .join(&artifact.version)
            .join(format!("{}.json", artifact.listing_digest));
        let listing_bytes = read_regular_file(&listing_path, artifact.listing_bytes)?;
        let listing = validate_listing_bytes(&listing_bytes, &artifact)?;
        artifacts.push(AdmittedArtifact {
            id: artifact.id,
            version: artifact.version,
            package_sha256: artifact.digest,
            package_size: artifact.bytes,
            package_path,
            manifest: manifest.catalog_value,
            listing,
        });
    }
    Ok(VerifiedAdmission { artifacts })
}

fn validate_version_policy(
    admissions_root: &Path,
    incoming: &Receipt,
) -> Result<(), AdmissionError> {
    for entry in fs::read_dir(admissions_root).map_err(|_| AdmissionError)? {
        let entry = entry.map_err(|_| AdmissionError)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| AdmissionError)?;
        let tree = name.strip_suffix(".tsv").ok_or(AdmissionError)?;
        if !valid_object_id(tree) || !entry.file_type().map_err(|_| AdmissionError)?.is_file() {
            return Err(AdmissionError);
        }
        let bytes = read_regular_file(&entry.path(), MAX_RECEIPT_BYTES as u64)?;
        let accepted = parse_unbound_receipt(&bytes)?;
        if accepted.review_tree != tree {
            return Err(AdmissionError);
        }
        for candidate in &incoming.artifacts {
            for existing in accepted
                .artifacts
                .iter()
                .filter(|artifact| artifact.id == candidate.id)
            {
                let existing_version =
                    Version::parse(&existing.version).map_err(|_| AdmissionError)?;
                let candidate_version =
                    Version::parse(&candidate.version).map_err(|_| AdmissionError)?;
                if existing_version > candidate_version
                    || (existing_version == candidate_version
                        && (existing.digest != candidate.digest
                            || existing.listing_digest != candidate.listing_digest))
                {
                    return Err(AdmissionError);
                }
            }
        }
    }
    Ok(())
}

fn validate_artifact_inventory(root: &Path, count: usize) -> Result<(), AdmissionError> {
    let actual = fs::read_dir(root)
        .map_err(|_| AdmissionError)?
        .map(|entry| {
            let entry = entry.map_err(|_| AdmissionError)?;
            if !entry.file_type().map_err(|_| AdmissionError)?.is_file() {
                return Err(AdmissionError);
            }
            entry.file_name().into_string().map_err(|_| AdmissionError)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut expected = BTreeSet::new();
    for index in 1..=count {
        expected.insert(format!("{index}.ocpkg"));
        expected.insert(format!("{index}.listing.json"));
    }
    if actual != expected {
        return Err(AdmissionError);
    }
    Ok(())
}

fn validate_package_bytes(
    bytes: &[u8],
    artifact: &Artifact,
) -> Result<package::InspectedManifest, AdmissionError> {
    if u64::try_from(bytes.len()).ok() != Some(artifact.bytes)
        || hex_digest(bytes) != artifact.digest
    {
        return Err(AdmissionError);
    }
    let manifest = package::inspect_bytes(bytes).map_err(|_| AdmissionError)?;
    if manifest.id != artifact.id || manifest.version != artifact.version {
        return Err(AdmissionError);
    }
    Ok(manifest)
}

fn validate_listing_bytes(
    bytes: &[u8],
    artifact: &Artifact,
) -> Result<package::Listing, AdmissionError> {
    if u64::try_from(bytes.len()).ok() != Some(artifact.listing_bytes)
        || hex_digest(bytes) != artifact.listing_digest
    {
        return Err(AdmissionError);
    }
    package::parse_listing_bytes(bytes).map_err(|_| AdmissionError)
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_object_id(value: &str) -> bool {
    valid_lower_hex(value, 40)
}

fn valid_digest(value: &str) -> bool {
    valid_lower_hex(value, 64)
}

fn valid_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_widget_source(value: &str) -> bool {
    value.strip_prefix("widgets/").is_some_and(|name| {
        !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _, path::Path};

    use super::{ExpectedAdmission, ingest, parse_receipt, verify};
    use crate::package;

    const TRUST_SHA: &str = "1111111111111111111111111111111111111111";
    const REVIEW_SHA: &str = "2222222222222222222222222222222222222222";
    const REVIEW_TREE: &str = "3333333333333333333333333333333333333333";
    const SECOND_TREE: &str = "4444444444444444444444444444444444444444";

    #[test]
    fn receipt_binds_exact_revisions_and_artifacts() {
        let receipt = format!(
            "admission\t2\t{TRUST_SHA}\t{REVIEW_SHA}\t{REVIEW_TREE}\n\
             artifact\twidgets/example\tcom.example.widget\t1.2.3\t{}\t4096\t{}\t512\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let expected = ExpectedAdmission {
            trust_sha: TRUST_SHA,
            review_sha: REVIEW_SHA,
            review_tree: REVIEW_TREE,
        };

        let parsed = parse_receipt(receipt.as_bytes(), &expected).expect("exact receipt");

        assert_eq!(parsed.artifacts.len(), 1);
        assert_eq!(parsed.artifacts[0].id, "com.example.widget");
        assert_eq!(parsed.artifacts[0].version, "1.2.3");
        assert_eq!(parsed.artifacts[0].bytes, 4096);
        assert_eq!(parsed.artifacts[0].listing_digest, "b".repeat(64));
        assert_eq!(parsed.artifacts[0].listing_bytes, 512);

        let wrong_review = ExpectedAdmission {
            review_sha: "4444444444444444444444444444444444444444",
            ..expected
        };
        assert!(parse_receipt(receipt.as_bytes(), &wrong_review).is_err());
        let obsolete = format!(
            "admission\t1\t{TRUST_SHA}\t{REVIEW_SHA}\t{REVIEW_TREE}\n\
             artifact\twidgets/example\tcom.example.widget\t1.2.3\t{}\t4096\n",
            "a".repeat(64)
        );
        assert!(parse_receipt(obsolete.as_bytes(), &expected).is_err());
    }

    #[test]
    fn ingestion_persists_the_exact_package_and_receipt() {
        let scratch = private_tempdir();
        let store = private_subdirectory(scratch.path(), "accepted");
        let input = admission_inputs(
            scratch.path(),
            "input",
            REVIEW_TREE,
            "1.0.0",
            b"<!doctype html><p>persistent</p>",
        );

        let admitted = ingest(
            &input.receipt,
            &input.artifacts,
            &store,
            &expected_for(REVIEW_TREE),
        )
        .unwrap();

        assert_eq!(admitted.artifact_count, 1);
        assert_eq!(admitted.review_tree, REVIEW_TREE);
        fs::write(&input.package, b"destroyed source copy").unwrap();
        let reopened = verify(&store, REVIEW_TREE).unwrap();
        let stored_package = store
            .join("packages/com.playervox.overcrow.hello/1.0.0")
            .join(format!("{}.ocpkg", input.digest));
        assert_eq!(fs::read(stored_package).unwrap(), input.bytes);
        let stored_listing = store
            .join("listings/com.playervox.overcrow.hello/1.0.0")
            .join(format!("{}.json", input.listing_digest));
        assert_eq!(fs::read(stored_listing).unwrap(), input.listing_bytes);
        assert_eq!(reopened.artifact_count, 1);
        assert_eq!(
            fs::read(store.join("admissions").join(format!("{REVIEW_TREE}.tsv"))).unwrap(),
            fs::read(input.receipt).unwrap()
        );
    }

    #[test]
    fn ingestion_rejects_tampered_package_bytes_without_a_completion_receipt() {
        let scratch = private_tempdir();
        let store = private_subdirectory(scratch.path(), "accepted");
        let input = admission_inputs(
            scratch.path(),
            "input",
            REVIEW_TREE,
            "1.0.0",
            b"<!doctype html><p>original</p>",
        );
        let mut package_bytes = input.bytes.clone();
        package_bytes[0] ^= 0xff;
        fs::write(&input.package, package_bytes).unwrap();

        assert!(
            ingest(
                &input.receipt,
                &input.artifacts,
                &store,
                &expected_for(REVIEW_TREE),
            )
            .is_err()
        );
        assert!(
            !store
                .join("admissions")
                .join(format!("{REVIEW_TREE}.tsv"))
                .exists()
        );
    }

    #[test]
    fn ingestion_rejects_same_size_listing_tampering_without_a_completion_receipt() {
        let scratch = private_tempdir();
        let store = private_subdirectory(scratch.path(), "accepted");
        let input = admission_inputs(
            scratch.path(),
            "input",
            REVIEW_TREE,
            "1.0.0",
            b"<!doctype html><p>original</p>",
        );
        let tampered = String::from_utf8(input.listing_bytes.clone())
            .unwrap()
            .replace("PlayerVox", "PlayerV0x");
        assert_eq!(tampered.len(), input.listing_bytes.len());
        fs::write(input.artifacts.join("1.listing.json"), tampered).unwrap();

        assert!(
            ingest(
                &input.receipt,
                &input.artifacts,
                &store,
                &expected_for(REVIEW_TREE),
            )
            .is_err()
        );
        assert!(
            !store
                .join("admissions")
                .join(format!("{REVIEW_TREE}.tsv"))
                .exists()
        );
    }

    #[test]
    fn ingestion_rejects_an_unreceipted_artifact() {
        let scratch = private_tempdir();
        let store = private_subdirectory(scratch.path(), "accepted");
        let input = admission_inputs(
            scratch.path(),
            "input",
            REVIEW_TREE,
            "1.0.0",
            b"<!doctype html><p>declared</p>",
        );
        fs::copy(&input.package, input.artifacts.join("2.ocpkg")).unwrap();

        assert!(
            ingest(
                &input.receipt,
                &input.artifacts,
                &store,
                &expected_for(REVIEW_TREE),
            )
            .is_err()
        );
        assert!(
            !store
                .join("admissions")
                .join(format!("{REVIEW_TREE}.tsv"))
                .exists()
        );
    }

    #[test]
    fn ingestion_rejects_same_version_replacement_and_downgrade() {
        for (first_version, second_version) in [("1.0.0", "1.0.0"), ("2.0.0", "1.0.0")] {
            let scratch = private_tempdir();
            let store = private_subdirectory(scratch.path(), "accepted");
            let first = admission_inputs(
                scratch.path(),
                "first",
                REVIEW_TREE,
                first_version,
                b"<!doctype html><p>first</p>",
            );
            ingest(
                &first.receipt,
                &first.artifacts,
                &store,
                &expected_for(REVIEW_TREE),
            )
            .unwrap();
            let second = admission_inputs(
                scratch.path(),
                "second",
                SECOND_TREE,
                second_version,
                b"<!doctype html><p>different bytes</p>",
            );

            assert!(
                ingest(
                    &second.receipt,
                    &second.artifacts,
                    &store,
                    &expected_for(SECOND_TREE),
                )
                .is_err()
            );
            assert!(
                !store
                    .join("admissions")
                    .join(format!("{SECOND_TREE}.tsv"))
                    .exists()
            );
        }
    }

    #[test]
    fn ingestion_rejects_same_version_listing_replacement() {
        let scratch = private_tempdir();
        let store = private_subdirectory(scratch.path(), "accepted");
        let first = admission_inputs(
            scratch.path(),
            "first",
            REVIEW_TREE,
            "1.0.0",
            b"<!doctype html><p>same package</p>",
        );
        ingest(
            &first.receipt,
            &first.artifacts,
            &store,
            &expected_for(REVIEW_TREE),
        )
        .unwrap();
        let second = admission_inputs(
            scratch.path(),
            "second",
            SECOND_TREE,
            "1.0.0",
            b"<!doctype html><p>same package</p>",
        );
        assert_eq!(second.digest, first.digest);
        let replacement = String::from_utf8(second.listing_bytes.clone())
            .unwrap()
            .replace("PlayerVox", "PlayerV0x");
        let replacement_digest = super::hex_digest(replacement.as_bytes());
        fs::write(second.artifacts.join("1.listing.json"), &replacement).unwrap();
        fs::write(
            &second.receipt,
            receipt_for(
                SECOND_TREE,
                "1.0.0",
                &second.digest,
                second.bytes.len(),
                &replacement_digest,
                replacement.len(),
            ),
        )
        .unwrap();

        assert!(
            ingest(
                &second.receipt,
                &second.artifacts,
                &store,
                &expected_for(SECOND_TREE),
            )
            .is_err()
        );
        assert!(
            !store
                .join("admissions")
                .join(format!("{SECOND_TREE}.tsv"))
                .exists()
        );
    }

    fn private_tempdir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    fn private_subdirectory(parent: &Path, name: &str) -> std::path::PathBuf {
        let directory = parent.join(name);
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }

    fn receipt_for(
        review_tree: &str,
        version: &str,
        digest: &str,
        bytes: usize,
        listing_digest: &str,
        listing_bytes: usize,
    ) -> Vec<u8> {
        format!(
            "admission\t2\t{TRUST_SHA}\t{REVIEW_SHA}\t{review_tree}\n\
             artifact\twidgets/hello-web\tcom.playervox.overcrow.hello\t{version}\t{digest}\t{bytes}\t{listing_digest}\t{listing_bytes}\n"
        )
        .into_bytes()
    }

    fn expected_for(review_tree: &'static str) -> ExpectedAdmission<'static> {
        ExpectedAdmission {
            trust_sha: TRUST_SHA,
            review_sha: REVIEW_SHA,
            review_tree,
        }
    }

    struct AdmissionInput {
        receipt: std::path::PathBuf,
        artifacts: std::path::PathBuf,
        package: std::path::PathBuf,
        digest: String,
        bytes: Vec<u8>,
        listing_digest: String,
        listing_bytes: Vec<u8>,
    }

    fn admission_inputs(
        parent: &Path,
        name: &str,
        review_tree: &str,
        version: &str,
        view: &[u8],
    ) -> AdmissionInput {
        let root = private_subdirectory(parent, name);
        let source = private_subdirectory(&root, "source");
        let artifacts = private_subdirectory(&root, "artifacts");
        fs::write(source.join("index.html"), view).unwrap();
        let listing_bytes = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/hello-web/listing.json"),
        )
        .unwrap();
        fs::write(source.join("listing.json"), &listing_bytes).unwrap();
        fs::write(
            source.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "id": "com.playervox.overcrow.hello",
                "version": version,
                "apiVersion": "1",
                "entrypoints": {"view": "index.html"},
                "permissions": {},
                "files": {
                    "index.html": {"sha256": super::hex_digest(view), "bytes": view.len()}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let package_path = artifacts.join("1.ocpkg");
        let written = package::write_package(&source, &package_path).unwrap();
        let package_bytes = fs::read(&package_path).unwrap();
        let digest = package::sha256_hex(&written.digest);
        let listing_path = artifacts.join("1.listing.json");
        let listing_digest = super::hex_digest(&listing_bytes);
        fs::write(&listing_path, &listing_bytes).unwrap();
        let receipt_path = root.join("admission.tsv");
        fs::write(
            &receipt_path,
            receipt_for(
                review_tree,
                version,
                &digest,
                package_bytes.len(),
                &listing_digest,
                listing_bytes.len(),
            ),
        )
        .unwrap();
        AdmissionInput {
            receipt: receipt_path,
            artifacts,
            package: package_path,
            digest,
            bytes: package_bytes,
            listing_digest,
            listing_bytes,
        }
    }
}
