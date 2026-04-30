use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Keypair, PublicKey, SecretKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::license::{
    ensure_sensitive_dir, validate_sensitive_file, write_atomic_secure, CmdResult, CommandError,
};

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
const INSTALLATION_KEY_FILE: &str = "installation.key";
const INSTALLATION_SCHEMA_V4: u32 = 4;
pub(crate) const OBSERVED_BINDING_CACHE_TTL: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct CachedObservedBinding {
    fingerprint: DeviceFingerprint,
    hardware_hash: [u8; 32],
    observed_at: Instant,
}

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
    key_path: PathBuf,
    installation: Arc<RwLock<InstallationState>>,
    observer: Arc<dyn HardwareObserver + Send + Sync>,
    /// Hardware hash anchored at installation time (content of `installation.json`).
    /// Never updated after initial bootstrap — used to detect hardware drift.
    persisted_hardware_hash: [u8; 32],
    observed_binding_cache: Arc<Mutex<Option<CachedObservedBinding>>>,
    observed_binding_cache_ttl: Duration,
}

impl std::fmt::Debug for DeviceBindingStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceBindingStore")
            .field("installation_path", &self.installation_path)
            .field("key_path", &self.key_path)
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
        Self::load_or_init_from_dir_with_observer_and_cache_ttl(
            dir,
            default_hardware_observer(),
            OBSERVED_BINDING_CACHE_TTL,
        )
    }

    #[cfg(test)]
    pub(crate) fn load_or_init_from_dir_with_observer(
        dir: impl AsRef<Path>,
        observer: Arc<dyn HardwareObserver + Send + Sync>,
    ) -> CmdResult<Self> {
        Self::load_or_init_from_dir_with_observer_and_cache_ttl(
            dir,
            observer,
            OBSERVED_BINDING_CACHE_TTL,
        )
    }

    pub(crate) fn load_or_init_from_dir_with_observer_and_cache_ttl(
        dir: impl AsRef<Path>,
        observer: Arc<dyn HardwareObserver + Send + Sync>,
        observed_binding_cache_ttl: Duration,
    ) -> CmdResult<Self> {
        let dir = dir.as_ref();
        ensure_sensitive_dir(dir).map_err(|e| CommandError::io(e.to_string()))?;
        let installation_path = dir.join(INSTALLATION_FILE);
        let key_path = dir.join(INSTALLATION_KEY_FILE);
        let installation = if installation_path.exists() {
            validate_sensitive_file(&installation_path)
                .map_err(|e| CommandError::io(e.to_string()))?;
            let bytes =
                fs::read(&installation_path).map_err(|e| CommandError::io(e.to_string()))?;
            let file_schema = read_schema(&bytes)?;
            let (state, needs_persist) =
                Self::load_existing(&installation_path, &key_path, &bytes, observer.as_ref())?;
            if needs_persist || file_schema < INSTALLATION_SCHEMA_V4 {
                tracing::info!(
                    schema = file_schema,
                    path = %installation_path.display(),
                    "Upgrading installation.json from schema {} to V4 and moving secret material to key store",
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
                    fingerprint: None,
                    hardware_hash: None,
                    legacy_device_hash: Some(hash),
                    migrated_from_legacy: true,
                })
            } else {
                None
            };
            let installation =
                Self::bootstrap_installation(legacy_context, &key_path, observer.as_ref())?;
            Self::persist(&installation_path, &installation)?;
            installation
        };

        // Capture the persisted hash before any in-memory refresh so we can track drift.
        let persisted_hardware_hash = installation.hardware_hash;

        let store = Self {
            installation_path,
            key_path,
            installation: Arc::new(RwLock::new(installation)),
            observer,
            persisted_hardware_hash,
            observed_binding_cache: Arc::new(Mutex::new(None)),
            observed_binding_cache_ttl,
        };
        store.refresh_observed_binding()?;
        Ok(store)
    }

    fn load_existing(
        _path: &Path,
        key_path: &Path,
        bytes: &[u8],
        observer: &dyn HardwareObserver,
    ) -> CmdResult<(InstallationState, bool)> {
        let schema = read_schema(bytes)?;
        if schema >= INSTALLATION_SCHEMA_V4 {
            let file: InstallationFileV4 =
                serde_json::from_slice(bytes).map_err(|e| CommandError::parse(e.to_string()))?;
            return Ok((file.try_into_state(key_path)?, false));
        }

        if schema >= 3 {
            let file: InstallationFileV3 =
                serde_json::from_slice(bytes).map_err(|e| CommandError::parse(e.to_string()))?;
            return Ok((file.try_into_state(key_path)?, true));
        }

        if schema >= 2 {
            let legacy: InstallationFileV2 =
                serde_json::from_slice(bytes).map_err(|e| CommandError::parse(e.to_string()))?;
            return Ok((legacy.into_context()?.into_state(key_path, observer)?, true));
        }

        let legacy: InstallationFileV1 =
            serde_json::from_slice(bytes).map_err(|e| CommandError::parse(e.to_string()))?;
        Ok((legacy.into_context()?.into_state(key_path, observer)?, true))
    }

    fn persist(path: &Path, installation: &InstallationState) -> CmdResult<()> {
        let file = InstallationFileV4::from_state(installation);
        let bytes =
            serde_json::to_vec_pretty(&file).map_err(|err| CommandError::parse(err.to_string()))?;
        write_atomic_secure(path, &bytes).map_err(|err| CommandError::io(err.to_string()))
    }

    fn bootstrap_installation(
        mut ctx: Option<LegacyBindingContext>,
        key_path: &Path,
        observer: &dyn HardwareObserver,
    ) -> CmdResult<InstallationState> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let installation_id = ctx
            .as_ref()
            .and_then(|c| c.installation_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let created_at = ctx.as_ref().and_then(|c| c.created_at).unwrap_or(now);
        let expected_keypair = ctx
            .as_mut()
            .and_then(|c| c.keypair.take())
            .map(Ok)
            .unwrap_or_else(generate_installation_keypair)?;
        let key_store = load_or_create_key_store(key_path, Some(expected_keypair))?;
        let installation_pubkey = key_store.pubkey_bytes();
        let fingerprint = match ctx.as_ref().and_then(|c| c.fingerprint.clone()) {
            Some(fingerprint) => fingerprint,
            None => collect_fingerprint_with_observer(&installation_id, observer)?,
        };
        let hardware_hash = match ctx.as_ref().and_then(|c| c.hardware_hash) {
            Some(hash) => hash,
            None => fingerprint_hardware_hash_bytes(&fingerprint)?,
        };
        let legacy_device_hash = ctx.as_ref().and_then(|c| c.legacy_device_hash);
        let migrated_from_legacy = ctx
            .as_ref()
            .map(|c| c.migrated_from_legacy || c.legacy_device_hash.is_some())
            .unwrap_or(false);

        Ok(InstallationState {
            installation_id,
            hardware_hash,
            installation_pubkey,
            key_store: Arc::new(key_store),
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
        let (observed, observed_hash) =
            self.cached_or_collect_observed_binding(&current.installation_id)?;

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

    pub fn invalidate_observed_binding_cache(&self) {
        let mut guard = self
            .observed_binding_cache
            .lock()
            .expect("observed binding cache poisoned");
        *guard = None;
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
    /// This is the indirection layer for installation signing. The current
    /// backend is a dedicated `installation.key` file; future phases may swap
    /// this to a platform keystore (Keychain / DPAPI / secret-service) without
    /// changing any callers.
    pub fn key_store(&self) -> Arc<dyn InstallationKeyStore + Send + Sync> {
        self.snapshot().key_store
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
        self.snapshot().key_store.sign(payload)
    }

    fn cached_or_collect_observed_binding(
        &self,
        installation_id: &str,
    ) -> CmdResult<(DeviceFingerprint, [u8; 32])> {
        let mut guard = self
            .observed_binding_cache
            .lock()
            .expect("observed binding cache poisoned");
        if let Some(cached) = guard.as_ref() {
            if cached.observed_at.elapsed() <= self.observed_binding_cache_ttl {
                return Ok((cached.fingerprint.clone(), cached.hardware_hash));
            }
        }

        let observed = collect_fingerprint_with_observer(installation_id, self.observer.as_ref())?;
        let observed_hash = fingerprint_hardware_hash_bytes(&observed)?;
        *guard = Some(CachedObservedBinding {
            fingerprint: observed.clone(),
            hardware_hash: observed_hash,
            observed_at: Instant::now(),
        });
        Ok((observed, observed_hash))
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
    fingerprint: Option<DeviceFingerprint>,
    hardware_hash: Option<[u8; 32]>,
    legacy_device_hash: Option<[u8; 32]>,
    migrated_from_legacy: bool,
}

impl LegacyBindingContext {
    fn into_state(
        self,
        key_path: &Path,
        observer: &dyn HardwareObserver,
    ) -> CmdResult<InstallationState> {
        DeviceBindingStore::bootstrap_installation(Some(self), key_path, observer)
    }
}

#[derive(Serialize, Deserialize)]
struct InstallationFileV4 {
    schema: u32,
    installation_id: String,
    installation_pubkey_b64: String,
    key_file: String,
    fingerprint: DeviceFingerprint,
    hardware_hash_hex: String,
    created_at: i64,
    migrated_from_legacy: bool,
    legacy_device_hash_hex: Option<String>,
}

impl InstallationFileV4 {
    fn from_state(state: &InstallationState) -> Self {
        Self {
            schema: INSTALLATION_SCHEMA_V4,
            installation_id: state.installation_id.clone(),
            installation_pubkey_b64: STANDARD.encode(state.installation_pubkey()),
            key_file: INSTALLATION_KEY_FILE.into(),
            fingerprint: state.fingerprint.clone(),
            hardware_hash_hex: state.device_hash_hex(),
            created_at: state.created_at,
            migrated_from_legacy: state.migrated_from_legacy,
            legacy_device_hash_hex: state.legacy_device_hash.map(hex::encode),
        }
    }

    fn try_into_state(self, key_path: &Path) -> CmdResult<InstallationState> {
        if self.schema != INSTALLATION_SCHEMA_V4 {
            return Err(CommandError::parse(format!(
                "installation schema {} unsupported",
                self.schema
            )));
        }
        let expected_pubkey = STANDARD
            .decode(&self.installation_pubkey_b64)
            .map_err(|err| CommandError::parse(err.to_string()))?;
        if expected_pubkey.len() != 32 {
            return Err(CommandError::parse(
                "installation public key must be 32 bytes",
            ));
        }
        let store = FileBackedKeyStore::open(key_path)?;
        if store.pubkey_bytes().as_slice() != expected_pubkey.as_slice() {
            return Err(CommandError::new(
                "InstallationKeyMismatch",
                format!(
                    "Installation key file {} does not match installation.json public key",
                    key_path.display()
                ),
            ));
        }
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
            installation_pubkey: store.pubkey_bytes(),
            key_store: Arc::new(store),
            fingerprint: self.fingerprint,
            created_at: self.created_at,
            migrated_from_legacy: true,
            legacy_device_hash,
        })
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
    fn try_into_state(self, key_path: &Path) -> CmdResult<InstallationState> {
        if self.schema != 3 {
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
        let store = load_or_create_key_store(key_path, Some(keypair))?;
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
            installation_pubkey: store.pubkey_bytes(),
            key_store: Arc::new(store),
            fingerprint: self.fingerprint,
            created_at: self.created_at,
            migrated_from_legacy: true,
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
            fingerprint: None,
            hardware_hash: None,
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
            fingerprint: None,
            hardware_hash: None,
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

fn load_or_create_key_store(
    path: &Path,
    expected_keypair: Option<Keypair>,
) -> CmdResult<FileBackedKeyStore> {
    if path.exists() {
        let store = FileBackedKeyStore::open(path)?;
        if let Some(keypair) = expected_keypair {
            if store.pubkey_bytes() != keypair.public.to_bytes() {
                return Err(CommandError::new(
                    "InstallationKeyMismatch",
                    format!(
                        "Installation key file {} does not match legacy key material in installation.json",
                        path.display()
                    ),
                ));
            }
        }
        return Ok(store);
    }

    let keypair = expected_keypair.unwrap_or(generate_installation_keypair()?);
    FileBackedKeyStore::create(path, &keypair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::license::runtime::fingerprint::{HardwareObserver, ObservedHardware};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

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

    #[derive(Clone)]
    struct CountingObserver {
        observed: ObservedHardware,
        calls: Arc<AtomicUsize>,
    }

    impl HardwareObserver for CountingObserver {
        fn observe(&self) -> CmdResult<ObservedHardware> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.observed.clone())
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
            disk_serial: Some(format!("disk-{machine_id}")),
            cpu_model: Some(cpu.into()),
            hostname: Some(hostname.into()),
            locale: Some("en_US.UTF-8".into()),
            timezone: "-0600".into(),
        }))
    }

    fn counting_observer(
        machine_id: &str,
        calls: Arc<AtomicUsize>,
    ) -> Arc<dyn HardwareObserver + Send + Sync> {
        Arc::new(CountingObserver {
            observed: ObservedHardware {
                platform: shared_core::Platform::Macos,
                machine_id: Some(machine_id.into()),
                disk_serial: Some(format!("disk-{machine_id}")),
                cpu_model: Some("cpu".into()),
                hostname: Some("host".into()),
                locale: Some("en_US.UTF-8".into()),
                timezone: "-0600".into(),
            },
            calls,
        })
    }

    #[test]
    fn creates_new_installation_when_missing() {
        let dir = temp_dir();
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("should init");
        let installation_json: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(INSTALLATION_FILE)).unwrap()).unwrap();
        assert_eq!(store.installation().migrated_from_legacy, false);
        assert!(!store.installation_id().is_empty());
        assert_ne!(store.device_hash(), [0u8; 32]);
        assert_ne!(store.installation_pubkey(), [0u8; 32]);
        shared_core::validate_fingerprint(&store.fingerprint()).expect("fingerprint valid");
        assert!(dir.join(INSTALLATION_FILE).exists());
        assert!(dir.join(INSTALLATION_KEY_FILE).exists());
        assert!(installation_json.get("keypair_b64").is_none());
        assert_eq!(
            installation_json
                .get("installation_pubkey_b64")
                .and_then(|value| value.as_str())
                .map(|value| !value.is_empty()),
            Some(true)
        );
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
        assert!(dir.join(INSTALLATION_KEY_FILE).exists());
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

        let persisted_before: InstallationFileV4 =
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
        let persisted_after: InstallationFileV4 =
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
    fn reload_preserves_installation_identity() {
        let dir = temp_dir();
        let first = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        let second = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("reload");
        assert_eq!(first.installation_id(), second.installation_id());
        assert_eq!(first.installation_pubkey(), second.installation_pubkey());
    }

    #[test]
    fn evidence_new_installation_filesystem_layout() {
        let dir = temp_dir();
        let first = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        let second = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("reload");
        let installation_path = dir.join(INSTALLATION_FILE);
        let key_path = dir.join(INSTALLATION_KEY_FILE);
        let installation_json: serde_json::Value =
            serde_json::from_slice(&fs::read(&installation_path).unwrap()).unwrap();
        let key_bytes = fs::read(&key_path).unwrap();

        println!("EVIDENCE_NEW_DIR={}", dir.display());
        println!(
            "EVIDENCE_NEW_INSTALLATION_JSON={}",
            installation_path.display()
        );
        println!("EVIDENCE_NEW_INSTALLATION_KEY={}", key_path.display());
        println!("EVIDENCE_NEW_INSTALLATION_ID={}", first.installation_id());
        println!(
            "EVIDENCE_NEW_INSTALLATION_PUBKEY={}",
            hex::encode(first.installation_pubkey())
        );

        assert!(installation_json.get("keypair_b64").is_none());
        assert!(installation_json.get("installation_pubkey_b64").is_some());
        assert_eq!(key_bytes.len(), 32);
        assert_eq!(first.installation_id(), second.installation_id());
        assert_eq!(first.installation_pubkey(), second.installation_pubkey());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
            println!("EVIDENCE_NEW_INSTALLATION_KEY_MODE={:o}", mode);
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn device_store_files_use_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir();
        let _ = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");

        let installation_mode =
            fs::metadata(dir.join(INSTALLATION_FILE)).unwrap().permissions().mode() & 0o777;
        let key_mode =
            fs::metadata(dir.join(INSTALLATION_KEY_FILE)).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;

        assert_eq!(installation_mode, 0o600);
        assert_eq!(key_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
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
        assert!(dir.join(INSTALLATION_KEY_FILE).exists());
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
        let installation_json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(store.installation_id(), "legacy-v2");
        assert_eq!(store.legacy_device_hash(), Some([0x22u8; 32]));
        assert_eq!(store.installation_pubkey(), keypair.public.to_bytes());
        assert_eq!(store.fingerprint().version, 2);
        assert!(dir.join(INSTALLATION_KEY_FILE).exists());
        assert!(installation_json.get("keypair_b64").is_none());
    }

    #[test]
    fn missing_key_file_fails_with_clear_error() {
        let dir = temp_dir();
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        assert_ne!(store.installation_pubkey(), [0u8; 32]);
        fs::remove_file(dir.join(INSTALLATION_KEY_FILE)).unwrap();

        let err = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .unwrap_err();
        assert_eq!(err.code, "MissingInstallationKey");
    }

    #[test]
    fn corrupt_key_file_fails_with_clear_error() {
        let dir = temp_dir();
        let _ = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .expect("init");
        fs::write(dir.join(INSTALLATION_KEY_FILE), [0x11u8; 31]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                dir.join(INSTALLATION_KEY_FILE),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }

        let err = DeviceBindingStore::load_or_init_from_dir_with_observer(
            &dir,
            observer("a", "cpu", "host"),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidInstallationKey");
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

    #[test]
    fn refresh_observed_binding_reuses_cache_within_ttl() {
        let dir = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer_and_cache_ttl(
            &dir,
            counting_observer("cached-machine", Arc::clone(&calls)),
            Duration::from_secs(10),
        )
        .expect("init");

        let baseline_calls = calls.load(Ordering::SeqCst);
        assert_eq!(
            baseline_calls, 2,
            "bootstrap + initial refresh should collect twice"
        );
        store
            .refresh_observed_binding()
            .expect("refresh within ttl");
        store
            .refresh_observed_binding()
            .expect("refresh within ttl again");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls,
            "refreshes inside the TTL must reuse the observed binding cache",
        );
    }

    #[test]
    fn refresh_observed_binding_recollects_after_ttl() {
        let dir = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let store = DeviceBindingStore::load_or_init_from_dir_with_observer_and_cache_ttl(
            &dir,
            counting_observer("ttl-machine", Arc::clone(&calls)),
            Duration::from_millis(25),
        )
        .expect("init");

        let baseline_calls = calls.load(Ordering::SeqCst);
        assert_eq!(
            baseline_calls, 2,
            "bootstrap + initial refresh should collect twice"
        );
        thread::sleep(Duration::from_millis(40));
        store.refresh_observed_binding().expect("refresh after ttl");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls + 1,
            "refresh after the TTL must recollect hardware",
        );
    }

    #[test]
    fn concurrent_refreshes_share_single_observation_within_ttl() {
        let dir = temp_dir();
        let calls = Arc::new(AtomicUsize::new(0));
        let store = Arc::new(
            DeviceBindingStore::load_or_init_from_dir_with_observer_and_cache_ttl(
                &dir,
                counting_observer("parallel-machine", Arc::clone(&calls)),
                Duration::from_secs(10),
            )
            .expect("init"),
        );

        store.invalidate_observed_binding_cache();
        let mut workers = Vec::new();
        for _ in 0..8 {
            let store = Arc::clone(&store);
            workers.push(thread::spawn(move || {
                store.refresh_observed_binding().expect("parallel refresh");
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }

        let baseline_calls = 2usize;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            baseline_calls + 1,
            "one extra observation should serve all parallel refreshes after invalidation",
        );
    }
}
