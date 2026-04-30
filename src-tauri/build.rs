use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const LICENSE_KEYRING_ENV: &str = "LICENSE_KEYRING_ENV";
const ED25519_SIGNATURE_ALG: &str = "ed25519-sha512";

#[derive(Debug, Deserialize)]
struct TrustManifest {
    #[serde(default)]
    keys: Vec<TrustAnchorRecord>,
    #[serde(default)]
    pending_environments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TrustKeyStatus {
    Active,
    Accepted,
    Deprecated,
    Retired,
}

impl TrustKeyStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Accepted => "accepted",
            Self::Deprecated => "deprecated",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TrustAnchorRecord {
    environment: String,
    key_id: String,
    key_version: Option<String>,
    algorithm: String,
    public_key_fingerprint_sha256: String,
    status: TrustKeyStatus,
    asset_path: Option<String>,
    #[allow(dead_code)]
    not_before: Option<String>,
    #[allow(dead_code)]
    not_after: Option<String>,
}

#[derive(Debug, Clone)]
struct EmbeddedTrustAnchor {
    record: TrustAnchorRecord,
    key_bytes: [u8; 32],
}

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=.sqlx");
    println!("cargo:rerun-if-changed=src/license/keys");
    println!("cargo:rerun-if-changed=src/license/keys/manifest.json");
    println!("cargo:rerun-if-env-changed={LICENSE_KEYRING_ENV}");

    generate_keyring_module();

    if env::var("SQLX_FORCE_OFFLINE").is_ok() {
        println!("cargo:rustc-env=SQLX_OFFLINE=true");
    }

    tauri_build::build()
}

fn generate_keyring_module() {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let selected_env = select_keyring_env(&profile).unwrap_or_else(|message| panic!("{message}"));
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let manifest_path = manifest_dir
        .join("src")
        .join("license")
        .join("keys")
        .join("manifest.json");
    let manifest = load_trust_manifest(&manifest_path);
    let trust_anchors = collect_trust_anchors(&manifest, &manifest_path, &profile, selected_env);
    let active = trust_anchors
        .iter()
        .filter(|entry| entry.record.status == TrustKeyStatus::Active)
        .collect::<Vec<_>>();
    if active.is_empty() {
        panic!("trust manifest {} has no active key for env={selected_env}", manifest_path.display());
    }
    if active.len() > 1 {
        panic!(
            "trust manifest {} has multiple active keys for env={selected_env}",
            manifest_path.display()
        );
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let generated_path = out_dir.join("license_keyring.rs");
    fs::write(
        &generated_path,
        render_generated_keyring(selected_env, active[0].record.key_id.as_str(), active[0].record.key_version.as_deref(), &trust_anchors),
    )
    .unwrap_or_else(|err| panic!("failed to write {}: {err}", generated_path.display()));
}

fn select_keyring_env(profile: &str) -> Result<&'static str, String> {
    match env::var(LICENSE_KEYRING_ENV) {
        Ok(value) => match value.as_str() {
            "dev" if profile == "release" => Err(format!(
                "{LICENSE_KEYRING_ENV}=dev is not allowed for release builds"
            )),
            "dev" => Ok("dev"),
            "staging" => Ok("staging"),
            "prod" => Ok("prod"),
            _ => Err(format!(
                "invalid {LICENSE_KEYRING_ENV}={value:?}; expected one of: dev, staging, prod"
            )),
        },
        Err(env::VarError::NotPresent) if profile == "release" => Err(format!(
            "release builds require {LICENSE_KEYRING_ENV}=staging or {LICENSE_KEYRING_ENV}=prod"
        )),
        Err(env::VarError::NotPresent) => Ok("dev"),
        Err(env::VarError::NotUnicode(_)) => {
            Err(format!("{LICENSE_KEYRING_ENV} must be valid Unicode"))
        }
    }
}

fn load_trust_manifest(manifest_path: &Path) -> TrustManifest {
    let manifest_raw = fs::read_to_string(manifest_path).unwrap_or_else(|err| {
        panic!(
            "failed to read trust manifest {}: {err}",
            manifest_path.display()
        )
    });
    serde_json::from_str(&manifest_raw).unwrap_or_else(|err| {
        panic!(
            "failed to parse trust manifest {}: {err}",
            manifest_path.display()
        )
    })
}

fn collect_trust_anchors(
    manifest: &TrustManifest,
    manifest_path: &Path,
    profile: &str,
    selected_env: &str,
) -> Vec<EmbeddedTrustAnchor> {
    let manifest_dir = manifest_path.parent().expect("manifest parent directory");
    let env_records = manifest
        .keys
        .iter()
        .filter(|entry| entry.environment == selected_env)
        .cloned()
        .collect::<Vec<_>>();

    if env_records.is_empty() {
        if manifest
            .pending_environments
            .iter()
            .any(|env_name| env_name == selected_env)
        {
            panic!(
                "trust anchors for env={selected_env} are pending provisioning in {}",
                manifest_path.display()
            );
        }
        panic!(
            "trust manifest {} has no key entries for env={selected_env}",
            manifest_path.display()
        );
    }

    env_records
        .into_iter()
        .map(|record| {
            validate_trust_anchor_metadata(&record, selected_env);
            let asset_path = record.asset_path.as_ref().unwrap_or_else(|| {
                panic!(
                    "trust anchor env={selected_env} key_id={} key_version={:?} missing asset_path",
                    record.key_id, record.key_version
                )
            });
            let key_path = manifest_dir.join(asset_path);
            let key_bytes = read_public_key(&key_path, profile, selected_env);
            validate_trust_anchor_fingerprint(&record, selected_env, &key_path, &key_bytes);
            EmbeddedTrustAnchor { record, key_bytes }
        })
        .collect()
}

fn validate_trust_anchor_metadata(trust_anchor: &TrustAnchorRecord, selected_env: &str) {
    if trust_anchor.key_id.trim().is_empty() {
        panic!("trust anchor for env={selected_env} has empty key_id");
    }
    if trust_anchor.algorithm != ED25519_SIGNATURE_ALG {
        panic!(
            "unsupported algorithm for env={selected_env}: expected {ED25519_SIGNATURE_ALG}, got {}",
            trust_anchor.algorithm
        );
    }
}

fn validate_trust_anchor_fingerprint(
    trust_anchor: &TrustAnchorRecord,
    selected_env: &str,
    key_path: &Path,
    key_bytes: &[u8; 32],
) {
    let fingerprint = fingerprint_sha256(key_bytes);
    if fingerprint != trust_anchor.public_key_fingerprint_sha256 {
        panic!(
            "fingerprint mismatch for env={selected_env} at {}: manifest={}, actual={}",
            key_path.display(),
            trust_anchor.public_key_fingerprint_sha256,
            fingerprint
        );
    }
}

fn read_public_key(path: &Path, profile: &str, selected_env: &str) -> [u8; 32] {
    let bytes = fs::read(path).unwrap_or_else(|err| {
        panic!(
            "missing license trust anchor for env={selected_env} profile={profile} at {}: {err}",
            path.display()
        )
    });
    let actual_len = bytes.len();
    <[u8; 32]>::try_from(bytes.as_slice()).unwrap_or_else(|_| {
        panic!(
            "invalid public key length for env={selected_env} at {}: expected 32 bytes, got {}",
            path.display(),
            actual_len
        )
    })
}

fn fingerprint_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn render_generated_keyring(
    selected_env: &str,
    active_key_id: &str,
    active_key_version: Option<&str>,
    trust_anchors: &[EmbeddedTrustAnchor],
) -> String {
    let records = trust_anchors
        .iter()
        .map(|entry| {
            let byte_list = entry
                .key_bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "    EmbeddedKeyRecord {{ key_id: {:?}, key_version: {:?}, status: {:?}, public_key: [{byte_list}] }},\n",
                entry.record.key_id,
                entry.record.key_version,
                entry.record.status.as_str(),
            )
        })
        .collect::<String>();

    format!(
        "pub struct EmbeddedKeyRecord {{\n\
    pub key_id: &'static str,\n\
    pub key_version: Option<&'static str>,\n\
    pub status: &'static str,\n\
    pub public_key: [u8; 32],\n\
}}\n\
pub const KEYRING_ENV: &str = {selected_env:?};\n\
pub const EMBEDDED_ACTIVE_KEY_ID: &str = {active_key_id:?};\n\
pub const EMBEDDED_ACTIVE_KEY_VERSION: Option<&str> = {active_key_version:?};\n\
pub static EMBEDDED_KEYS: &[EmbeddedKeyRecord] = &[\n\
{records}];\n"
    )
}
