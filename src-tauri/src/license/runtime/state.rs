use crate::license::runtime::fingerprint::DeviceFingerprint;
use ed25519_dalek::Keypair;
use hex;
use std::fmt;

/// Installation identity persisted on disk so the client can prove
/// its binding without regenerating identifiers on each boot.
pub struct InstallationState {
    pub installation_id: String,
    pub hardware_hash: [u8; 32],
    pub keypair: Keypair,
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
        self.keypair.public.to_bytes()
    }

    #[allow(dead_code)]
    pub fn fingerprint(&self) -> &DeviceFingerprint {
        &self.fingerprint
    }

    pub fn refresh_observed_binding(
        &mut self,
        fingerprint: DeviceFingerprint,
        hardware_hash: [u8; 32],
    ) -> bool {
        let changed = self.hardware_hash != hardware_hash || self.fingerprint != fingerprint;
        self.hardware_hash = hardware_hash;
        self.fingerprint = fingerprint;
        changed
    }
}

impl Clone for InstallationState {
    fn clone(&self) -> Self {
        let keypair =
            Keypair::from_bytes(&self.keypair.to_bytes()).expect("clone installation keypair");
        Self {
            installation_id: self.installation_id.clone(),
            hardware_hash: self.hardware_hash,
            keypair,
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
