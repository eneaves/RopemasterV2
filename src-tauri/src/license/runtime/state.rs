use crate::license::runtime::fingerprint::DeviceFingerprint;
use hex;
use std::fmt;
use std::sync::Arc;

use super::key_store::InstallationKeyStore;

/// Installation identity persisted on disk so the client can prove
/// its binding without regenerating identifiers on each boot.
pub struct InstallationState {
    pub installation_id: String,
    pub hardware_hash: [u8; 32],
    pub installation_pubkey: [u8; 32],
    pub key_store: Arc<dyn InstallationKeyStore + Send + Sync>,
    pub fingerprint: DeviceFingerprint,
    pub created_at: i64,
    pub migrated_from_legacy: bool,
    pub legacy_device_hash: Option<[u8; 32]>,
}

impl InstallationState {
    pub fn device_hash(&self) -> [u8; 32] {
        self.hardware_hash
    }

    pub fn device_hash_hex(&self) -> String {
        hex::encode(self.hardware_hash)
    }

    pub fn installation_pubkey(&self) -> [u8; 32] {
        self.installation_pubkey
    }

    #[allow(dead_code)]
    pub fn fingerprint(&self) -> &DeviceFingerprint {
        &self.fingerprint
    }

}

impl Clone for InstallationState {
    fn clone(&self) -> Self {
        Self {
            installation_id: self.installation_id.clone(),
            hardware_hash: self.hardware_hash,
            installation_pubkey: self.installation_pubkey,
            key_store: Arc::clone(&self.key_store),
            fingerprint: self.fingerprint.clone(),
            created_at: self.created_at,
            migrated_from_legacy: self.migrated_from_legacy,
            legacy_device_hash: self.legacy_device_hash,
        }
    }
}

impl fmt::Debug for InstallationState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstallationState")
            .field("installation_id", &self.installation_id)
            .field("binding_hash_hex", &self.device_hash_hex())
            .field("created_at", &self.created_at)
            .field("migrated_from_legacy", &self.migrated_from_legacy)
            .finish()
    }
}
