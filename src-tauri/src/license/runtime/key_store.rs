use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Keypair, PublicKey, SecretKey};
use sha2::{Digest, Sha256};

use crate::license::{ensure_sensitive_dir, CmdResult, CommandError};

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

/// Abstraction for the installation's private key.
///
/// Implementations must never expose raw secret bytes to callers. The only
/// supported operation is signing payloads and exposing the derived verifying
/// key for identity checks.
pub trait InstallationKeyStore: Send + Sync {
    fn pubkey_bytes(&self) -> [u8; 32];
    fn sign(&self, payload: &[u8]) -> [u8; 64];
    fn derive_secret(&self, purpose: &[u8], context: &[u8]) -> Vec<u8>;
}

/// Persists only the 32-byte Ed25519 seed in a separate file with restrictive
/// permissions. The JSON installation state keeps only public metadata.
pub struct FileBackedKeyStore {
    path: PathBuf,
    keypair: Keypair,
}

impl FileBackedKeyStore {
    pub fn create(path: impl AsRef<Path>, keypair: &Keypair) -> CmdResult<Self> {
        let path = path.as_ref().to_path_buf();
        write_seed_file(&path, &keypair.secret.to_bytes())?;
        Self::open(path)
    }

    pub fn open(path: impl AsRef<Path>) -> CmdResult<Self> {
        let path = path.as_ref().to_path_buf();
        let seed = read_seed_file(&path)?;
        let keypair = keypair_from_seed(&seed)?;
        Ok(Self { path, keypair })
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

    fn derive_secret(&self, purpose: &[u8], context: &[u8]) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(b"roping-manager-installation-secret-v1");
        hasher.update(purpose);
        hasher.update(context);
        hasher.update(self.keypair.public.to_bytes());
        hasher.update(self.keypair.secret.to_bytes());
        hasher.finalize().to_vec()
    }
}

impl std::fmt::Debug for FileBackedKeyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileBackedKeyStore")
            .field("path", &self.path)
            .field("pubkey", &hex::encode(self.pubkey_bytes()))
            .finish()
    }
}

fn keypair_from_seed(seed: &[u8; 32]) -> CmdResult<Keypair> {
    let secret = SecretKey::from_bytes(seed).map_err(|err| CommandError::parse(err.to_string()))?;
    let public: PublicKey = (&secret).into();
    Ok(Keypair { secret, public })
}

fn read_seed_file(path: &Path) -> CmdResult<[u8; 32]> {
    validate_seed_path(path)?;
    let bytes = fs::read(path).map_err(|err| CommandError::io(err.to_string()))?;
    if bytes.len() != 32 {
        return Err(CommandError::new(
            "InvalidInstallationKey",
            format!(
                "Installation key file {} must contain exactly 32 bytes",
                path.display()
            ),
        ));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

fn write_seed_file(path: &Path, seed: &[u8; 32]) -> CmdResult<()> {
    if let Some(parent) = path.parent() {
        ensure_sensitive_dir(parent).map_err(|err| CommandError::io(err.to_string()))?;
    }
    if path.exists() {
        return Err(CommandError::new(
            "InstallationKeyExists",
            format!("Installation key file already exists at {}", path.display()),
        ));
    }

    let tmp_path = tmp_key_path(path);
    let file = create_seed_tmp_file(&tmp_path)?;
    write_seed_tmp_file(file, seed, &tmp_path)?;
    fs::rename(&tmp_path, path).map_err(|err| CommandError::io(err.to_string()))?;
    validate_seed_path(path)?;
    Ok(())
}

fn create_seed_tmp_file(path: &Path) -> CmdResult<File> {
    #[cfg(unix)]
    {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|err| CommandError::io(err.to_string()))
    }

    #[cfg(not(unix))]
    {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|err| CommandError::io(err.to_string()))
    }
}

fn write_seed_tmp_file(mut file: File, seed: &[u8; 32], path: &Path) -> CmdResult<()> {
    file.write_all(seed)
        .and_then(|_| file.sync_all())
        .map_err(|err| CommandError::io(err.to_string()))?;
    drop(file);
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|err| CommandError::io(err.to_string()))?;
    }
    Ok(())
}

fn tmp_key_path(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let suffix = format!(".tmp-{}-{}", std::process::id(), uuid::Uuid::new_v4());
    let mut file_name = path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| "installation.key".into());
    file_name.push(suffix);
    tmp.set_file_name(file_name);
    tmp
}

fn validate_seed_path(path: &Path) -> CmdResult<()> {
    let meta = fs::symlink_metadata(path).map_err(|err| {
        CommandError::new(
            "MissingInstallationKey",
            format!("Installation key file {} is missing: {err}", path.display()),
        )
    })?;
    if meta.file_type().is_symlink() {
        return Err(CommandError::new(
            "InvalidInstallationKey",
            format!(
                "Installation key path {} must not be a symlink",
                path.display()
            ),
        ));
    }
    if !meta.is_file() {
        return Err(CommandError::new(
            "InvalidInstallationKey",
            format!(
                "Installation key path {} is not a regular file",
                path.display()
            ),
        ));
    }
    #[cfg(unix)]
    {
        let mode = meta.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(CommandError::new(
                "InsecureInstallationKeyPermissions",
                format!(
                    "Installation key file {} has insecure permissions {:o}; expected 600",
                    path.display(),
                    mode
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{FileBackedKeyStore, InstallationKeyStore};
    use ed25519_dalek::{Keypair, PublicKey, SecretKey};
    use std::fs;
    use std::path::PathBuf;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("installation-key-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_keypair() -> Keypair {
        let secret = SecretKey::from_bytes(&[0xAB; 32]).expect("secret key");
        let public: PublicKey = (&secret).into();
        Keypair { secret, public }
    }

    #[test]
    fn create_and_open_roundtrip() {
        let dir = temp_dir();
        let path = dir.join("device").join("installation.key");
        let kp = test_keypair();
        let created = FileBackedKeyStore::create(&path, &kp).expect("create key store");
        let reopened = FileBackedKeyStore::open(&path).expect("open key store");
        assert_eq!(created.pubkey_bytes(), kp.public.to_bytes());
        assert_eq!(reopened.pubkey_bytes(), kp.public.to_bytes());
        assert_eq!(fs::read(&path).unwrap().len(), 32);
    }

    #[cfg(unix)]
    #[test]
    fn create_sets_restrictive_file_and_parent_permissions() {
        let dir = temp_dir();
        let path = dir.join("device").join("installation.key");
        let kp = test_keypair();
        FileBackedKeyStore::create(&path, &kp).expect("create key store");

        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn sign_produces_verifiable_signature() {
        let dir = temp_dir();
        let path = dir.join("installation.key");
        let kp = test_keypair();
        let public = kp.public;
        let store = FileBackedKeyStore::create(&path, &kp).expect("create key store");
        let payload = b"phase4-binding-policy-test";
        let sig_bytes = store.sign(payload);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes).expect("parse sig");
        public
            .verify_strict(payload, &sig)
            .expect("signature valid");
    }

    #[test]
    fn rejects_missing_key_file() {
        let dir = temp_dir();
        let path = dir.join("installation.key");
        let err = FileBackedKeyStore::open(&path).unwrap_err();
        assert_eq!(err.code, "MissingInstallationKey");
    }

    #[test]
    fn rejects_corrupt_key_file() {
        let dir = temp_dir();
        let path = dir.join("installation.key");
        fs::write(&path, [0x11; 31]).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let err = FileBackedKeyStore::open(&path).unwrap_err();
        assert_eq!(err.code, "InvalidInstallationKey");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_insecure_permissions() {
        let dir = temp_dir();
        let path = dir.join("installation.key");
        fs::write(&path, [0x11; 32]).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let err = FileBackedKeyStore::open(&path).unwrap_err();
        assert_eq!(err.code, "InsecureInstallationKeyPermissions");
    }

    #[test]
    fn derive_secret_is_stable_and_context_bound() {
        let dir = temp_dir();
        let path = dir.join("installation.key");
        let kp = test_keypair();
        let store = FileBackedKeyStore::create(&path, &kp).expect("create key store");

        let first = store.derive_secret(b"license-snapshot", b"installation-a");
        let second = store.derive_secret(b"license-snapshot", b"installation-a");
        let other_context = store.derive_secret(b"license-snapshot", b"installation-b");
        let other_purpose = store.derive_secret(b"request-signing", b"installation-a");

        assert_eq!(first, second);
        assert_ne!(first, other_context);
        assert_ne!(first, other_purpose);
        assert_eq!(first.len(), 32);
    }
}
