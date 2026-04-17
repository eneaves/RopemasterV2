use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::license::{write_atomic, CmdResult, CommandError};

use super::{
    fingerprint::{
        collect_fingerprint_with_observer, default_hardware_observer,
        fingerprint_hardware_hash_bytes, DeviceFingerprint, HardwareObserver,
    },
    key_store::{FileBackedKeyStore, InstallationKeyStore},
    state::InstallationState,
};

const DEVICE_DIR: &str = "device";
const LEGACY_FILE: &str = "device_id.bin";
const INSTALLATION_FILE: &str = "installation.json";
const INSTALLATION_SCHEMA_V3: u32 = 3;

/// Manages the persistent installation identity and device binding.
///
/// # Phase 3 binding policy
///
/// `refresh_observed_binding` updates the **in-memory** hardware hash to the
/// current live observation so that validation always uses fresh data.
/// It does **not** rewrite `installation.json` when the observed hardware
/// differs from the persisted anchor.
///
/// If hardware has changed since the installation was anchored:
/// - `has_hardware_drift()` returns `true`
/// - the license evaluator will produce `BindingMatch::Mismatch`
/// - `installation.json` is **not** automatically updated
/// - the user must generate a new `.req` to obtain a license for the new hardware
#[derive(Clone)]
pub struct DeviceBindingStore {
    installation_path: PathBuf,
    installation: Arc<RwLock<InstallationState>>,
    observer: Arc<dyn HardwareObserver + Send + Sync>,
    /// Hardware hash anchored at installation time (content of `installation.json`).
    /// Never updated after initial bootstrap — used to detect hardware drift.
    persisted_hardware_hash: [u8; 32],
}

impl std::fmt::Debug for DeviceBindingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceBindingStore")
            .field("installation_path", &self.installation_path)
            .field("installation", &self.installation())
            .field("has_hardware_drift", &self.has_hardware_drift())
            .finish()
    }
}

impl DeviceBindingStore {
    pub fn load_or_init(app: &AppHandle) -> CmdResult<Self> {
        let dir = app
            .path()
            .app_config_dir()
            .map_err(|e| CommandError::io(e.to_string()))?
            .join(DEVICE_DIR);
        Self::load_or_init_from_dir(dir)
    }

    pub(crate) fn load_or_init_from_dir(dir: impl AsRef<Path>) -> CmdResult<Self> {
        Self::load_or_init_from_dir_with_observer(dir, default_hardware_observer())
    }

    pub(crate) fn load_or_init_from_dir_with_observer(
        dir: impl AsRef<Path>,
        observer: Arc<dyn HardwareObserver + Send + Sync>,
    ) -> CmdResult<Self> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| CommandError::io(e.to_string()))?;
        let installation_path = dir.join(INSTALLATION_FILE);
        let installation = if installation_path.exists() {
            // Read the schema before loading so we know whether to upgrade.
            let file_schema = {
                let bytes = fs::read(&installation_path)
                    .map_err(|e| CommandError::io(e.to_string()))?;
                read_schema(&bytes)?
            };
            let state = Self::load_existing(&installation_path, observer.as_ref())?;
            if file_schema < INSTALLATION_SCHEMA_V3 {
                // Eagerly upgrade legacy schemas (V1/V2) to V3 so the keypair
                // is stable across restarts.
                //
                // V1 files do not store the private key → without this persist a new
                // random keypair would be generated on *every* startup, making every
                // previously-issued license produce DeviceMismatch on the next boot.
                //
                // V2 files preserve the existing keypair; upgrading is safe and
                // prevents the unnecessary re-bootstrap overhead on every run.
                //
                // This is a one-time migration: once the file is V3 it will be loaded
                // directly on all subsequent runs.
                tracing::info!(
                    schema = file_schema,
                    path = %installation_path.display(),
                    "Upgrading installation.json from schema {} to V3 to stabilise identity",
                    file_schema,
                );
                Self::persist(&installation_path, &state)?;
            }
            state
        } else {
            let legacy_path = dir.join(LEGACY_FILE);
            let legacy_context = if let Some(hash) = Self::load_legacy_hash(&legacy_path)? {
                Some(LegacyBindingContext {
                    installation_id: None,
                    created_at: None,
                    keypair: None,
                    legacy_device_hash: Some(hash),
                    migrated_from_legacy: true,
                })
            } else {
                None
            };
            let installation = Self::bootstrap_installation(legacy_context, observer.as_ref())?;
            Self::persist(&installation_path, &installation)?;
            installation
        };

        // Capture the persisted hash before any in-memory refresh so we can track drift.
        let persisted_hardware_hash = installation.hardware_hash;

        let store = Self {
            installation_path,
            installation: Arc::new(RwLock::new(installation)),
            observer,
            persisted_hardware_hash,
        };
        store.refresh_observed_binding()?;
        Ok(store)
    }

    fn load_existing(path: &Path, observer: &dyn HardwareObserver) -> CmdResult<InstallationState> {
        let bytes = fs::read(path).map_err(|e| CommandError::io(e.to_string()))?;
        let schema = read_schema(&bytes)?;
        if schema >= INSTALLATION_SCHEMA_V3 {
            let file: InstallationFileV3 =
                serde_json::from_slice(&bytes).map_err(|e| CommandError::parse(e.to_string()))?;
            return file.try_into_state();
        }

        if schema >= 2 {
            let legacy: InstallationFileV2 =
                serde_json::from_slice(&bytes).map_err(|e| CommandError::parse(e.to_string()))?;
            return legacy.into_context()?.into_state(observer);
        }

        let legacy: InstallationFileV1 =
            serde_json::from_slice(&bytes).map_err(|e| CommandError::parse(e.to_string()))?;
        legacy.into_context()?.into_state(observer)
    }

    fn persist(path: &Path, installation: &InstallationState) -> CmdResult<()> {
        let file = InstallationFileV3::from_state(installation);
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|err| CommandError::parse(err.to_string()))?;
        write_atomic(path, &bytes).map_err(|err| CommandError::io(err.to_string()))
    }

    fn bootstrap_installation(
        mut ctx: Option<LegacyBindingContext>,
        observer: &dyn HardwareObserver,
    ) -> CmdResult<InstallationState> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let installation_id = ctx
            .as_ref()
            .and_then(|c| c.installation_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let created_at = ctx.as_ref().and_then(|c| c.created_at).unwrap_or(now);
        let keypair = ctx
            .as_mut()
            .and_then(|c| c.keypair.take())
            .unwrap_or(generate_installation_keypair()?);
        let fingerprint = collect_fingerprint_with_observer(&installation_id, observer)?;
        let hardware_hash = fingerprint_hardware_hash_bytes(&fingerprint)?;
        let legacy_device_hash = ctx.as_ref().and_then(|c| c.legacy_device_hash);
        let migrated_from_legacy = ctx
            .as_ref()
            .map(|c| c.migrated_from_legacy || c.legacy_device_hash.is_some())
            .unwrap_or(false);

        Ok(InstallationState {
            installation_id,
            hardware_hash,
            keypair,
            fingerprint,
            created_at,
            migrated_from_legacy,
            legacy_device_hash,
        })
    }

    fn load_legacy_hash(path: &Path) -> CmdResult<Option<[u8; 32]>> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path).map_err(|e| CommandError::io(e.to_string()))?;
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(Some(array))
        } else {
            Ok(None)
        }
    }

    fn snapshot(&self) -> InstallationState {
        self.installation
            .read()
            .expect("installation state poisoned")
            .clone()
    }

    /// Refreshes the **in-memory** binding with the current hardware observation.
    ///
    /// # Phase 3 policy
    ///
    /// This method **never** rewrites `installation.json` — the persisted anchor
    /// is immutable after initial bootstrap.
    ///
    /// If the observed hardware differs from the persisted anchor, the in-memory
    /// hash is updated so that license validation uses the current (mismatching)
    /// hardware hash.  Callers check `has_hardware_drift()` to detect this.
    ///
    /// If the hardware observation is insufficient (e.g. `machine_id` is missing),
    /// this method returns an error — it fails closed and does **not** update any
    /// state with a partial observation.
    pub fn refresh_observed_binding(&self) -> CmdResult<bool> {
        let current = self.snapshot();
        let observed =
            collect_fingerprint_with_observer(&current.installation_id, self.observer.as_ref())?;
        let observed_hash = fingerprint_hardware_hash_bytes(&observed)?;

        let changed = current.hardware_hash != observed_hash || current.fingerprint != observed;
        if changed {
            let mut next = current.clone();
            next.hardware_hash = observed_hash;
            next.fingerprint = observed;

            tracing::warn!(
                installation_id = %next.installation_id,
                persisted_hash = %hex::encode(self.persisted_hardware_hash),
                observed_hash = %hex::encode(observed_hash),
                "Observed hardware differs from persisted anchor; in-memory binding updated. \
                 installation.json is NOT rewritten — user must generate a new .req."
            );

            let mut guard = self
                .installation
                .write()
                .expect("installation state poisoned");
            *guard = next;
            // Phase 3 policy: do NOT call Self::persist here.
        }

        Ok(changed)
    }

    /// Returns `true` when the currently observed hardware hash differs from the
    /// hardware hash that was anchored at installation time (`installation.json`).
    ///
    /// When this returns `true` any installed license will evaluate as
    /// `BindingMatch::Mismatch` and the user must generate a new `.req`.
    pub fn has_hardware_drift(&self) -> bool {
        self.snapshot().hardware_hash != self.persisted_hardware_hash
    }

    /// Returns an abstraction over the installation's private key.
    ///
    /// This is the Phase 3 indirection layer.  Currently backed by the keypair
    /// stored in `installation.json`; future phases may swap this to a platform
    /// keystore (Keychain / DPAPI / secret-service) without changing any callers.
    pub fn key_store(&self) -> Arc<dyn InstallationKeyStore + Send + Sync> {
        let kp_bytes = self.snapshot().keypair.to_bytes();
        Arc::new(FileBackedKeyStore::new(
            Keypair::from_bytes(&kp_bytes).expect("clone installation keypair"),
        ))
    }

    #[allow(dead_code)]
    pub fn installation(&self) -> InstallationState {
        self.snapshot()
    }

    pub fn device_hash(&self) -> [u8; 32] {
        self.snapshot().device_hash()
    }

    pub fn legacy_device_hash(&self) -> Option<[u8; 32]> {
        self.snapshot().legacy_device_hash
    }

    pub fn device_hash_hex(&self) -> String {
        self.snapshot().device_hash_hex()
    }

    pub fn installation_id(&self) -> String {
        self.snapshot().installation_id
    }

    pub fn installation_pubkey(&self) -> [u8; 32] {
        self.snapshot().installation_pubkey()
    }

    pub fn fingerprint(&self) -> DeviceFingerprint {
        self.snapshot().fingerprint().clone()
    }

    pub fn sign_request(&self, payload: &[u8]) -> [u8; 64] {
        self.snapshot().keypair.sign(payload).to_bytes()
    }
}

fn read_schema(bytes: &[u8]) -> CmdResult<u32> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| CommandError::parse(e.to_string()))?;
    Ok(value
        .get("schema")
        .and_then(|v| v.as_u64())
        .map(|s| s as u32)
        .unwrap_or(1))
}

struct LegacyBindingContext {
    installation_id: Option<String>,
    created_at: Option<i64>,
    keypair: Option<Keypair>,
    legacy_device_hash: Option<[u8; 32]>,
    migrated_from_legacy: bool,
}

impl LegacyBindingContext {
    fn into_state(self, observer: &dyn HardwareObserver) -> CmdResult<InstallationState> {
        DeviceBindingStore::bootstrap_installation(Some(self), observer)
    }
}

#[derive(Serialize, Deserialize)]
struct InstallationFileV3 {
    schema: u32,
    installation_id: String,
    keypair_b64: String,
    fingerprint: DeviceFingerprint,
    hardware_hash_hex: String,
    created_at: i64,
    migrated_from_legacy: bool,
    legacy_device_hash_hex: Option<String>,
}

impl InstallationFileV3 {
    fn from_state(state: &InstallationState) -> Self {
        Self {
            schema: INSTALLATION_SCHEMA_V3,
            installation_id: state.installation_id.clone(),
            keypair_b64: STANDARD.encode(state.keypair.to_bytes()),
            fingerprint: state.fingerprint.clone(),
            hardware_hash_hex: state.device_hash_hex(),
            created_at: state.created_at,
            migrated_from_legacy: state.migrated_from_legacy,
            legacy_device_hash_hex: state.legacy_device_hash.map(hex::encode),
        }
    }

    fn try_into_state(self) -> CmdResult<InstallationState> {
        if self.schema != INSTALLATION_SCHEMA_V3 {
            return Err(CommandError::parse(format!(
                "installation schema {} unsupported",
                self.schema
            )));
        }
        let key_bytes = STANDARD
            .decode(self.keypair_b64)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        if key_bytes.len() != 64 {
            return Err(CommandError::parse("installation keypair must be 64 bytes"));
        }
        let keypair =
            Keypair::from_bytes(&key_bytes).map_err(|err| CommandError::parse(err.to_string()))?;
        shared_core::validate_fingerprint(&self.fingerprint)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        if self.hardware_hash_hex != self.fingerprint.hardware_hash {
            return Err(CommandError::parse(
                "hardware hash does not match fingerprint",
            ));
        }
        let hardware_hash = decode_hash_hex(&self.hardware_hash_hex)?;
        let legacy_device_hash = match self.legacy_device_hash_hex {
            Some(hex) => Some(decode_hash_hex(&hex)?),
            None => None,
        };
        Ok(InstallationState {
            installation_id: self.installation_id,
            hardware_hash,
            keypair,
            fingerprint: self.fingerprint,
            created_at: self.created_at,
            migrated_from_legacy: self.migrated_from_legacy,
            legacy_device_hash,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct InstallationFileV2 {
    schema: u32,
    installation_id: String,
    keypair_b64: String,
    #[allow(dead_code)]
    fingerprint: Value,
    binding_hash_hex: String,
    created_at: i64,
    migrated_from_legacy: bool,
    legacy_device_hash_hex: Option<String>,
}

impl InstallationFileV2 {
    fn into_context(self) -> CmdResult<LegacyBindingContext> {
        if self.schema != 2 {
            return Err(CommandError::parse(format!(
                "legacy installation schema {} unsupported",
                self.schema
            )));
        }
        let key_bytes = STANDARD
            .decode(self.keypair_b64)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        if key_bytes.len() != 64 {
            return Err(CommandError::parse("installation keypair must be 64 bytes"));
        }
        let keypair =
            Keypair::from_bytes(&key_bytes).map_err(|err| CommandError::parse(err.to_string()))?;
        let legacy_device_hash = match self.legacy_device_hash_hex {
            Some(hex) => Some(decode_hash_hex(&hex)?),
            None => Some(decode_hash_hex(&self.binding_hash_hex)?),
        };
        Ok(LegacyBindingContext {
            installation_id: Some(self.installation_id),
            created_at: Some(self.created_at),
            keypair: Some(keypair),
            legacy_device_hash,
            migrated_from_legacy: true,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct InstallationFileV1 {
    schema: u32,
    installation_id: String,
    device_hash_hex: String,
    installation_pubkey_b64: Option<String>,
    created_at: i64,
    migrated_from_legacy: bool,
}

impl InstallationFileV1 {
    fn into_context(self) -> CmdResult<LegacyBindingContext> {
        if self.schema != 1 {
            return Err(CommandError::parse(format!(
                "legacy installation schema {} unsupported",
                self.schema
            )));
        }
        let device_hash = decode_hash_hex(&self.device_hash_hex)?;
        Ok(LegacyBindingContext {
            installation_id: Some(self.installation_id),
            created_at: Some(self.created_at),
            keypair: None,
            legacy_device_hash: Some(device_hash),
            migrated_from_legacy: true,
        })
    }
}

fn decode_hash_hex(input: &str) -> CmdResult<[u8; 32]> {
    let decoded = hex::decode(input).map_err(|err| CommandError::parse(err.to_string()))?;
    if decoded.len() != 32 {
        return Err(CommandError::parse("binding hash must be 32 bytes"));
    }
    let mut array = [0u8; 32];
    array.copy_from_slice(&decoded);
    Ok(array)
}

fn generate_installation_keypair() -> CmdResult<Keypair> {
    let mut rng = OsRng;
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);
    let secret =
        SecretKey::from_bytes(&seed).map_err(|err| CommandError::parse(err.to_string()))?;
    let public: PublicKey = (&secret).into();
    Ok(Keypair { secret, public })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::license::runtime::fingerprint::{HardwareObserver, ObservedHardware};

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!("license-device-binding-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[derive(Clone)]
    struct FixedObserver(ObservedHardware);

    impl HardwareObserver for FixedObserver {
        fn observe(&self) -> CmdResult<ObservedHardware> {
            Ok(self.0.clone())
        }
    }

    fn observer(
        machine_id: &str,
        cpu: &str,
        hostname: &str,
    ) -> Arc<dyn HardwareObserver + Send + Sync> {
        Arc::new(FixedObserver(ObservedHardware {
            platform: shared_core::Platform::Macos,
            machine_id: Some(machine_id.into()),
            cpu_model: Some(cpu.into()),
            hostname: Some(hostname.into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }))
    }

    #[test]
    fn creates_new_installation_when_missing() {
        let dir = temp_dir();
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("should init");
        assert_eq!(store.installation().migrated_from_legacy, false);
        assert!(!store.installation_id().is_empty());
        assert_ne!(store.device_hash(), [0u8; 32]);
        assert_ne!(store.installation_pubkey(), [0u8; 32]);
        shared_core::validate_fingerprint(&store.fingerprint()).expect("fingerprint valid");
        assert!(dir.join(INSTALLATION_FILE).exists());
    }

    #[test]
    fn migrates_from_legacy_device_id() {
        let dir = temp_dir();
        let legacy = dir.join(LEGACY_FILE);
        let bytes = [0xABu8; 32];
        fs::write(&legacy, &bytes).unwrap();

        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("should init");
        assert!(store.installation().migrated_from_legacy);
        assert_eq!(store.legacy_device_hash(), Some(bytes));
        assert_ne!(store.device_hash(), bytes);
        assert!(dir.join(INSTALLATION_FILE).exists());
    }

    /// Phase 3 policy: when hardware changes, installation.json is NOT rewritten.
    /// The in-memory (observed) hash changes but the persisted file stays anchored.
    #[test]
    fn hardware_change_does_not_rewrite_installation_json() {
        let dir = temp_dir();
        let first = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        let first_hash = first.device_hash_hex();
        let installation_id = first.installation_id();

        let persisted_before: InstallationFileV3 =
            serde_json::from_slice(&fs::read(dir.join(INSTALLATION_FILE)).unwrap()).unwrap();
        assert_eq!(persisted_before.hardware_hash_hex, first_hash);

        // Load with different hardware
        let second = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("b", "cpu", "host"),
        )
        .expect("reload with different hardware");
        let second_hash = second.device_hash_hex();

        assert_eq!(installation_id, second.installation_id());
        assert_ne!(first_hash, second_hash);
        assert!(
            second.has_hardware_drift(),
            "hardware drift should be detected"
        );

        // Phase 3 policy: installation.json must NOT have been updated
        let persisted_after: InstallationFileV3 =
            serde_json::from_slice(&fs::read(dir.join(INSTALLATION_FILE)).unwrap()).unwrap();
        assert_eq!(
            persisted_after.hardware_hash_hex, first_hash,
            "installation.json must NOT be rewritten when hardware changes"
        );
    }

    #[test]
    fn no_hardware_drift_when_hardware_unchanged() {
        let dir = temp_dir();
        let _ = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        let store2 = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("reload same hardware");
        assert!(!store2.has_hardware_drift());
    }

    #[test]
    fn key_store_signs_and_pubkey_matches_installation_pubkey() {
        let dir = temp_dir();
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        let key_store = store.key_store();
        assert_eq!(key_store.pubkey_bytes(), store.installation_pubkey());
        let sig = key_store.sign(b"test-payload");
        let pubkey = ed25519_dalek::PublicKey::from_bytes(&store.installation_pubkey()).unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&sig).unwrap();
        pubkey
            .verify_strict(b"test-payload", &signature)
            .expect("key_store signature valid");
    }

    #[test]
    fn migrates_schema_v1_file() {
        let dir = temp_dir();
        let legacy_file = InstallationFileV1 {
            schema: 1,
            installation_id: "legacy-id".into(),
            device_hash_hex: hex::encode([0x11u8; 32]),
            installation_pubkey_b64: None,
            created_at: 1_700_000_000,
            migrated_from_legacy: false,
        };
        let path = dir.join(INSTALLATION_FILE);
        let payload = serde_json::to_vec(&legacy_file).unwrap();
        fs::write(&path, payload).unwrap();

        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("migrate");
        assert!(store.installation().migrated_from_legacy);
        assert_eq!(store.legacy_device_hash(), Some([0x11u8; 32]));
        assert_eq!(store.installation_id(), "legacy-id");
        assert_eq!(store.fingerprint().version, 2);
    }

    #[test]
    fn migrates_schema_v2_file() {
        let dir = temp_dir();
        let keypair = generate_installation_keypair().unwrap();
        let path = dir.join(INSTALLATION_FILE);
        let legacy_file = InstallationFileV2 {
            schema: 2,
            installation_id: "legacy-v2".into(),
            keypair_b64: STANDARD.encode(keypair.to_bytes()),
            fingerprint: serde_json::json!({"schema":1}),
            binding_hash_hex: hex::encode([0x22u8; 32]),
            created_at: 1_700_000_000,
            migrated_from_legacy: false,
            legacy_device_hash_hex: None,
        };
        fs::write(&path, serde_json::to_vec(&legacy_file).unwrap()).unwrap();

        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("migrate");
        assert_eq!(store.installation_id(), "legacy-v2");
        assert_eq!(store.legacy_device_hash(), Some([0x22u8; 32]));
        assert_eq!(store.installation_pubkey(), keypair.public.to_bytes());
        assert_eq!(store.fingerprint().version, 2);
    }

    #[test]
    fn errors_on_malformed_file() {
        let dir = temp_dir();
        let path = dir.join(INSTALLATION_FILE);
        fs::write(&path, b"not-json").unwrap();
        let err = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .unwrap_err();
        assert_eq!(err.code, "Parse");
    }
}
