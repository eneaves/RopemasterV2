use std::collections::HashMap;
use std::sync::Arc;

use ed25519_dalek::PublicKey;
use once_cell::sync::Lazy;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/license_keyring.rs"));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    Active,
    Accepted,
    Deprecated,
    Retired,
}

impl KeyStatus {
    pub fn from_manifest_str(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "accepted" => Self::Accepted,
            "deprecated" => Self::Deprecated,
            "retired" => Self::Retired,
            other => panic!("unsupported embedded key status: {other}"),
        }
    }

    pub fn allows_verification(self) -> bool {
        !matches!(self, Self::Retired)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyLookupError {
    UnknownKeyId { key_id: String },
    KeyVersionMismatch {
        key_id: String,
        key_version: Option<String>,
    },
    RetiredKey {
        key_id: String,
        key_version: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedKey {
    pub public_key: PublicKey,
}

pub const DEFAULT_KEY_ID: &str = generated::EMBEDDED_ACTIVE_KEY_ID;
pub const DEFAULT_KEY_VERSION: Option<&str> = generated::EMBEDDED_ACTIVE_KEY_VERSION;
pub const KEYRING_ENV: &str = generated::KEYRING_ENV;

pub trait LicenseKeyring: Send + Sync {
    fn active_key(&self) -> PublicKey;
    fn resolve_key(&self, key_id: &str) -> Option<PublicKey>;

    fn resolve_key_versioned(&self, key_id: &str, _key_version: Option<&str>) -> Option<PublicKey> {
        self.resolve_key(key_id)
    }

    fn lookup_key(
        &self,
        key_id: &str,
        key_version: Option<&str>,
    ) -> Result<ResolvedKey, KeyLookupError> {
        let Some(public_key) = self.resolve_key_versioned(key_id, key_version) else {
            return if self.resolve_key(key_id).is_some() {
                Err(KeyLookupError::KeyVersionMismatch {
                    key_id: key_id.to_string(),
                    key_version: key_version.map(str::to_string),
                })
            } else {
                Err(KeyLookupError::UnknownKeyId {
                    key_id: key_id.to_string(),
                })
            };
        };

        Ok(ResolvedKey {
            public_key,
        })
    }
}

#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub public_key: PublicKey,
    pub key_version: Option<String>,
    pub status: KeyStatus,
}

pub struct MultiKeyring {
    keys: HashMap<String, Vec<KeyEntry>>,
    active_key_id: String,
    active_key_version: Option<String>,
}

impl MultiKeyring {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            active_key_id: DEFAULT_KEY_ID.to_string(),
            active_key_version: DEFAULT_KEY_VERSION.map(str::to_string),
        }
    }

    #[cfg(test)]
    pub fn with_key(
        self,
        key_id: impl Into<String>,
        public_key: PublicKey,
        key_version: Option<String>,
    ) -> Self {
        self.with_key_status(key_id, public_key, key_version, KeyStatus::Active)
    }

    pub fn with_key_status(
        mut self,
        key_id: impl Into<String>,
        public_key: PublicKey,
        key_version: Option<String>,
        status: KeyStatus,
    ) -> Self {
        let id = key_id.into();
        self.keys.entry(id).or_default().push(KeyEntry {
            public_key,
            key_version,
            status,
        });
        self
    }

    pub fn with_active_key_versioned(
        mut self,
        key_id: impl Into<String>,
        key_version: Option<String>,
    ) -> Self {
        self.active_key_id = key_id.into();
        self.active_key_version = key_version;
        self
    }

    fn entries_for(&self, key_id: &str) -> Option<&[KeyEntry]> {
        self.keys.get(key_id).map(Vec::as_slice)
    }
}

impl LicenseKeyring for MultiKeyring {
    fn active_key(&self) -> PublicKey {
        self.lookup_key(&self.active_key_id, self.active_key_version.as_deref())
            .map(|entry| entry.public_key)
            .expect("active key id missing from keyring")
    }

    fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
        self.entries_for(key_id)
            .and_then(|entries| entries.first())
            .map(|entry| entry.public_key)
    }

    fn resolve_key_versioned(&self, key_id: &str, key_version: Option<&str>) -> Option<PublicKey> {
        self.lookup_key(key_id, key_version)
            .ok()
            .map(|entry| entry.public_key)
    }

    fn lookup_key(
        &self,
        key_id: &str,
        key_version: Option<&str>,
    ) -> Result<ResolvedKey, KeyLookupError> {
        let Some(entries) = self.entries_for(key_id) else {
            return Err(KeyLookupError::UnknownKeyId {
                key_id: key_id.to_string(),
            });
        };

        let entry = entries
            .iter()
            .find(|entry| entry.key_version.as_deref() == key_version)
            .or_else(|| entries.iter().find(|entry| entry.key_version.is_none()));
        let Some(entry) = entry else {
            return Err(KeyLookupError::KeyVersionMismatch {
                key_id: key_id.to_string(),
                key_version: key_version.map(str::to_string),
            });
        };

        if !entry.status.allows_verification() {
            return Err(KeyLookupError::RetiredKey {
                key_id: key_id.to_string(),
                key_version: entry.key_version.clone(),
            });
        }

        Ok(ResolvedKey {
            public_key: entry.public_key,
        })
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub struct EmbeddedKeyring;

static EMBEDDED_ACTIVE_KEY: Lazy<PublicKey> = Lazy::new(|| {
    PublicKey::from_bytes(
        &generated::EMBEDDED_KEYS
            .iter()
            .find(|entry| {
                entry.key_id == generated::EMBEDDED_ACTIVE_KEY_ID
                    && entry.key_version == generated::EMBEDDED_ACTIVE_KEY_VERSION
            })
            .expect("embedded active key missing from generated keyring")
            .public_key,
    )
    .expect("invalid embedded active license public key")
});

#[cfg(test)]
impl LicenseKeyring for EmbeddedKeyring {
    fn active_key(&self) -> PublicKey {
        *EMBEDDED_ACTIVE_KEY
    }

    fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
        generated::EMBEDDED_KEYS
            .iter()
            .find(|entry| entry.key_id == key_id)
            .map(|entry| {
                PublicKey::from_bytes(&entry.public_key)
                    .expect("invalid embedded license public key")
            })
    }

    fn resolve_key_versioned(&self, key_id: &str, key_version: Option<&str>) -> Option<PublicKey> {
        self.lookup_key(key_id, key_version)
            .ok()
            .map(|entry| entry.public_key)
    }

    fn lookup_key(
        &self,
        key_id: &str,
        key_version: Option<&str>,
    ) -> Result<ResolvedKey, KeyLookupError> {
        let has_key_id = generated::EMBEDDED_KEYS.iter().any(|entry| entry.key_id == key_id);
        if !has_key_id {
            return Err(KeyLookupError::UnknownKeyId {
                key_id: key_id.to_string(),
            });
        }
        let entry = generated::EMBEDDED_KEYS
            .iter()
            .filter(|entry| entry.key_id == key_id)
            .find(|entry| entry.key_version == key_version)
            .or_else(|| {
                generated::EMBEDDED_KEYS
                    .iter()
                    .filter(|entry| entry.key_id == key_id)
                    .find(|entry| entry.key_version.is_none())
            });
        let Some(entry) = entry else {
            return Err(KeyLookupError::KeyVersionMismatch {
                key_id: key_id.to_string(),
                key_version: key_version.map(str::to_string),
            });
        };
        let status = KeyStatus::from_manifest_str(entry.status);
        if !status.allows_verification() {
            return Err(KeyLookupError::RetiredKey {
                key_id: key_id.to_string(),
                key_version: entry.key_version.map(str::to_string),
            });
        }
        Ok(ResolvedKey {
            public_key: PublicKey::from_bytes(&entry.public_key)
                .expect("invalid embedded license public key"),
        })
    }
}

pub fn default_keyring() -> Arc<dyn LicenseKeyring + Send + Sync> {
    static KEYRING: Lazy<Arc<dyn LicenseKeyring + Send + Sync>> = Lazy::new(|| {
        let keyring = generated::EMBEDDED_KEYS.iter().fold(
            MultiKeyring::new().with_active_key_versioned(
                DEFAULT_KEY_ID,
                DEFAULT_KEY_VERSION.map(str::to_string),
            ),
            |ring, entry| {
                ring.with_key_status(
                    entry.key_id,
                    PublicKey::from_bytes(&entry.public_key)
                        .expect("invalid embedded license public key"),
                    entry.key_version.map(str::to_string),
                    KeyStatus::from_manifest_str(entry.status),
                )
            },
        );
        Arc::new(keyring)
    });
    KEYRING.clone()
}

pub fn embedded_public_key() -> PublicKey {
    *EMBEDDED_ACTIVE_KEY
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SecretKey;

    use super::*;

    fn make_pubkey(seed: u8) -> PublicKey {
        let secret = SecretKey::from_bytes(&[seed; 32]).unwrap();
        (&secret).into()
    }

    #[test]
    fn multi_keyring_resolves_known_key_id() {
        let key = make_pubkey(0x01);
        let keyring = MultiKeyring::new().with_key("primary", key, None);
        assert_eq!(keyring.resolve_key("primary"), Some(key));
    }

    #[test]
    fn multi_keyring_rejects_unknown_key_id() {
        let key = make_pubkey(0x02);
        let keyring = MultiKeyring::new().with_key("primary", key, None);
        assert_eq!(keyring.resolve_key("rotated"), None);
    }

    #[test]
    fn multi_keyring_resolves_multiple_key_versions_for_same_key_id() {
        let old = make_pubkey(0x03);
        let new = make_pubkey(0x04);
        let keyring = MultiKeyring::new()
            .with_key_status(
                "primary",
                old,
                Some("2026-04".into()),
                KeyStatus::Accepted,
            )
            .with_key_status("primary", new, Some("2026-10".into()), KeyStatus::Active);
        assert_eq!(
            keyring
                .lookup_key("primary", Some("2026-04"))
                .expect("accepted key")
                .public_key,
            old
        );
        assert_eq!(
            keyring
                .lookup_key("primary", Some("2026-10"))
                .expect("active key")
                .public_key,
            new
        );
    }

    #[test]
    fn multi_keyring_active_key_returns_selected_version() {
        let old = make_pubkey(0x05);
        let new = make_pubkey(0x06);
        let keyring = MultiKeyring::new()
            .with_key_status(
                "primary",
                old,
                Some("2026-04".into()),
                KeyStatus::Accepted,
            )
            .with_key_status("primary", new, Some("2026-10".into()), KeyStatus::Active)
            .with_active_key_versioned("primary", Some("2026-10".into()));
        assert_eq!(keyring.active_key(), new);
    }

    #[test]
    #[should_panic(expected = "active key id missing from keyring")]
    fn multi_keyring_active_key_panics_when_id_missing() {
        let keyring = MultiKeyring {
            keys: HashMap::new(),
            active_key_id: "nonexistent".to_string(),
            active_key_version: None,
        };
        let _ = keyring.active_key();
    }

    #[test]
    fn versioned_resolution_accepts_matching_version() {
        let key = make_pubkey(0x07);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        assert_eq!(
            keyring.resolve_key_versioned("primary", Some("2026")),
            Some(key)
        );
    }

    #[test]
    fn versioned_resolution_rejects_wrong_version() {
        let key = make_pubkey(0x08);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        assert_eq!(keyring.resolve_key_versioned("primary", Some("2025")), None);
    }

    #[test]
    fn versioned_resolution_rejects_missing_version_when_required() {
        let key = make_pubkey(0x09);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        assert_eq!(keyring.resolve_key_versioned("primary", None), None);
    }

    #[test]
    fn versioned_resolution_accepts_any_version_when_not_required() {
        let key = make_pubkey(0x0A);
        let keyring = MultiKeyring::new().with_key("primary", key, None);
        assert_eq!(
            keyring.resolve_key_versioned("primary", Some("anything")),
            Some(key)
        );
        assert_eq!(keyring.resolve_key_versioned("primary", None), Some(key));
    }

    #[test]
    fn lookup_rejects_retired_key_explicitly() {
        let key = make_pubkey(0x0B);
        let keyring = MultiKeyring::new().with_key_status(
            "primary",
            key,
            Some("2026-01".into()),
            KeyStatus::Retired,
        );
        match keyring.lookup_key("primary", Some("2026-01")) {
            Err(KeyLookupError::RetiredKey {
                key_id,
                key_version,
            }) if key_id == "primary" && key_version.as_deref() == Some("2026-01") => {}
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn default_keyring_contains_embedded_active_key() {
        let ring = default_keyring();
        let active = ring
            .lookup_key(DEFAULT_KEY_ID, DEFAULT_KEY_VERSION)
            .expect("embedded active key");
        assert_eq!(active.public_key, embedded_public_key());
    }

    #[test]
    fn embedded_keyring_active_matches_generated_constants() {
        let ring = EmbeddedKeyring;
        assert_eq!(ring.active_key(), embedded_public_key());
        assert_eq!(KEYRING_ENV, generated::KEYRING_ENV);
        assert!(!generated::EMBEDDED_KEYS.is_empty());
        assert!(generated::EMBEDDED_KEYS.iter().all(|entry| entry.status != "invalid"));
    }

    #[test]
    fn generated_keyring_contains_only_selected_environment_entries() {
        let versions = generated::EMBEDDED_KEYS
            .iter()
            .map(|entry| entry.key_version)
            .collect::<Vec<_>>();
        match KEYRING_ENV {
            "dev" => {
                assert_eq!(generated::EMBEDDED_KEYS.len(), 1);
                assert_eq!(versions, vec![None]);
            }
            "staging" => {
                assert_eq!(generated::EMBEDDED_KEYS.len(), 3);
                assert!(versions.contains(&Some("2026-04")));
                assert!(versions.contains(&Some("2026-07")));
                assert!(versions.contains(&Some("2026-10")));
            }
            "prod" => {
                assert_eq!(generated::EMBEDDED_KEYS.len(), 4);
                assert!(versions.contains(&Some("2026-01")));
                assert!(versions.contains(&Some("2026-04")));
                assert!(versions.contains(&Some("2026-07")));
                assert!(versions.contains(&Some("2026-10")));
            }
            other => panic!("unexpected keyring env {other}"),
        }
    }
}
