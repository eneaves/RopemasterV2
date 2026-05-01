/// Abstraction for the installation's private key.
///
/// Phase 3 introduces this trait to decouple the rest of the runtime from the
/// concrete key-storage backend. The current implementation (`FileBackedKeyStore`)
/// keeps the keypair in memory loaded from `installation.json`.
///
/// Future migration path (Phase 4+): replace `FileBackedKeyStore` with a
/// platform keystore backend (macOS Keychain / Windows DPAPI / secret-service)
/// without touching any of the callers.
///
/// Restrictions:
/// - implementations must be `Send + Sync` (the runtime crosses thread bounds),
/// - implementations must **not** expose the raw private key bytes to callers,
/// - signing is the only operation this trait surface allows.
pub trait InstallationKeyStore: Send + Sync {
    /// Returns the 32-byte compressed Ed25519 verifying (public) key.
    fn pubkey_bytes(&self) -> [u8; 32];

    /// Signs `payload` with the installation's private key and returns 64-byte signature.
    fn sign(&self, payload: &[u8]) -> [u8; 64];
}

/// File-backed implementation: the keypair lives in `installation.json` as
/// a 64-byte base64-encoded seed+public-key blob (ed25519-dalek v1 format).
///
/// This implementation is the current backend for all installations.
/// It purposely does NOT derive `Clone` — callers that need shared ownership
/// should wrap it in `Arc<dyn InstallationKeyStore>`.
pub struct FileBackedKeyStore {
    keypair: ed25519_dalek::Keypair,
}

impl FileBackedKeyStore {
    /// Wraps an already-decoded keypair. Use `DeviceBindingStore` to obtain
    /// the keypair from persistent storage.
    pub fn new(keypair: ed25519_dalek::Keypair) -> Self {
        Self { keypair }
    }
}

impl InstallationKeyStore for FileBackedKeyStore {
    fn pubkey_bytes(&self) -> [u8; 32] {
        self.keypair.public.to_bytes()
    }

    fn sign(&self, payload: &[u8]) -> [u8; 64] {
        use ed25519_dalek::Signer as _;
        self.keypair.sign(payload).to_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{FileBackedKeyStore, InstallationKeyStore};
    use ed25519_dalek::{Keypair, PublicKey, SecretKey, Verifier as _};

    fn test_keypair() -> Keypair {
        let secret = SecretKey::from_bytes(&[0xAB; 32]).expect("secret key");
        let public: PublicKey = (&secret).into();
        Keypair { secret, public }
    }

    #[test]
    fn pubkey_matches_keypair() {
        let kp = test_keypair();
        let expected = kp.public.to_bytes();
        let store = FileBackedKeyStore::new(kp);
        assert_eq!(store.pubkey_bytes(), expected);
    }

    #[test]
    fn sign_produces_verifiable_signature() {
        let kp = test_keypair();
        let public = kp.public;
        let store = FileBackedKeyStore::new(kp);
        let payload = b"phase3-binding-policy-test";
        let sig_bytes = store.sign(payload);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes).expect("parse sig");
        public
            .verify_strict(payload, &sig)
            .expect("signature valid");
    }

    #[test]
    fn sign_different_payloads_produce_different_signatures() {
        let kp = test_keypair();
        let store = FileBackedKeyStore::new(kp);
        let s1 = store.sign(b"payload-one");
        let s2 = store.sign(b"payload-two");
        assert_ne!(s1, s2);
    }

    #[test]
    fn file_backed_store_satisfies_trait_object_bounds() {
        // Ensure Box<dyn InstallationKeyStore> + Send + Sync compiles.
        let kp = test_keypair();
        let store: Box<dyn InstallationKeyStore> = Box::new(FileBackedKeyStore::new(kp));
        let _ = store.pubkey_bytes();
    }
}
