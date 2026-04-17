use std::collections::HashMap;
use std::sync::Arc;

use ed25519_dalek::PublicKey;
use once_cell::sync::Lazy;

pub const DEFAULT_KEY_ID: &str = "primary";

static EMBEDDED_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let bytes = include_bytes!("../public_key_dev.der");
    PublicKey::from_bytes(bytes).expect("invalid embedded license public key")
});

/// Resolución de llaves públicas para verificar licencias.
///
/// - `resolve_key`: resolución básica por `key_id`.
/// - `resolve_key_versioned`: resolución que también valida `key_version` si el keyring
///   lo requiere. La impl. por defecto ignora `key_version` y delega en `resolve_key`.
pub trait LicenseKeyring: Send + Sync {
    fn active_key(&self) -> PublicKey;
    fn resolve_key(&self, key_id: &str) -> Option<PublicKey>;

    /// Resuelve la llave pública para `key_id` y opcionalmente valida `key_version`.
    ///
    /// Si el keyring tiene una entrada con `key_version` requerida, la licencia debe
    /// declarar exactamente esa versión. Si la entrada no tiene `key_version` requerida,
    /// el campo en la licencia se ignora.
    ///
    /// Implementaciones que no soporten `key_version` pueden usar el default, que
    /// simplemente delega en `resolve_key`.
    fn resolve_key_versioned(&self, key_id: &str, _key_version: Option<&str>) -> Option<PublicKey> {
        self.resolve_key(key_id)
    }
}

/// Entrada individual en un `MultiKeyring`.
#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub public_key: PublicKey,
    /// Si se establece, las licencias que usen este `key_id` deben declarar exactamente
    /// este `key_version`; de lo contrario se rechaza en `resolve_key_versioned`.
    pub key_version: Option<String>,
}

/// Keyring multi-llave: resuelve llaves públicas por `key_id`, opcionalmente con
/// validación de `key_version`. Reemplaza al `EmbeddedKeyring` en la ruta de producción.
///
/// # Ejemplo
/// ```rust,ignore
/// let keyring = MultiKeyring::new()
///     .with_key("primary", dev_pubkey, None)
///     .with_key("primary-v2", prod_pubkey, Some("2026".into()));
/// ```
pub struct MultiKeyring {
    keys: HashMap<String, KeyEntry>,
    active_key_id: String,
}

impl MultiKeyring {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            active_key_id: DEFAULT_KEY_ID.to_string(),
        }
    }

    /// Registra una llave pública con la `key_id` dada y una `key_version` opcional.
    pub fn with_key(
        mut self,
        key_id: impl Into<String>,
        public_key: PublicKey,
        key_version: Option<String>,
    ) -> Self {
        let id = key_id.into();
        self.keys.insert(
            id,
            KeyEntry {
                public_key,
                key_version,
            },
        );
        self
    }

    /// Establece la `key_id` que retorna `active_key()`.
    pub fn with_active_key(mut self, key_id: impl Into<String>) -> Self {
        self.active_key_id = key_id.into();
        self
    }
}

impl LicenseKeyring for MultiKeyring {
    fn active_key(&self) -> PublicKey {
        self.keys
            .get(&self.active_key_id)
            .map(|e| e.public_key)
            .unwrap_or(*EMBEDDED_KEY)
    }

    fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
        self.keys.get(key_id).map(|e| e.public_key)
    }

    /// Si la entrada requiere un `key_version`, la versión de la licencia debe coincidir.
    /// Si la entrada no requiere versión, cualquier valor en la licencia se acepta.
    fn resolve_key_versioned(&self, key_id: &str, key_version: Option<&str>) -> Option<PublicKey> {
        let entry = self.keys.get(key_id)?;
        if let Some(required) = &entry.key_version {
            if key_version != Some(required.as_str()) {
                return None;
            }
        }
        Some(entry.public_key)
    }
}

/// Keyring single-llave legado. Mantenido para compatibilidad con tests existentes.
/// Para nuevos usos, preferir `MultiKeyring`.
#[derive(Debug, Clone)]
pub struct EmbeddedKeyring;

impl LicenseKeyring for EmbeddedKeyring {
    fn active_key(&self) -> PublicKey {
        *EMBEDDED_KEY
    }

    fn resolve_key(&self, key_id: &str) -> Option<PublicKey> {
        (key_id == DEFAULT_KEY_ID).then_some(*EMBEDDED_KEY)
    }
}

/// Retorna el keyring por defecto: `MultiKeyring` pre-cargado con la llave dev embebida
/// bajo `key_id = "primary"` (sin restricción de `key_version`).
pub fn default_keyring() -> Arc<dyn LicenseKeyring + Send + Sync> {
    static KEYRING: Lazy<Arc<dyn LicenseKeyring + Send + Sync>> =
        Lazy::new(|| Arc::new(MultiKeyring::new().with_key(DEFAULT_KEY_ID, *EMBEDDED_KEY, None)));
    KEYRING.clone()
}

pub fn embedded_public_key() -> PublicKey {
    *EMBEDDED_KEY
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SecretKey;

    use super::*;

    fn make_pubkey(seed: u8) -> PublicKey {
        let secret = SecretKey::from_bytes(&[seed; 32]).unwrap();
        (&secret).into()
    }

    // ── MultiKeyring: resolución básica ──────────────────────────────────────

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
    fn multi_keyring_resolves_multiple_keys() {
        let k1 = make_pubkey(0x03);
        let k2 = make_pubkey(0x04);
        let keyring =
            MultiKeyring::new()
                .with_key("primary", k1, None)
                .with_key("secondary", k2, None);
        assert_eq!(keyring.resolve_key("primary"), Some(k1));
        assert_eq!(keyring.resolve_key("secondary"), Some(k2));
        assert_ne!(k1, k2);
    }

    // ── MultiKeyring: active_key ─────────────────────────────────────────────

    #[test]
    fn multi_keyring_active_key_returns_first_registered_by_default() {
        let key = make_pubkey(0x05);
        let keyring = MultiKeyring::new()
            .with_key(DEFAULT_KEY_ID, key, None)
            .with_active_key(DEFAULT_KEY_ID);
        assert_eq!(keyring.active_key(), key);
    }

    #[test]
    fn multi_keyring_active_key_falls_back_to_embedded_when_id_missing() {
        // active_key_id no está en el mapa → fallback a EMBEDDED_KEY
        let keyring = MultiKeyring {
            keys: HashMap::new(),
            active_key_id: "nonexistent".to_string(),
        };
        assert_eq!(keyring.active_key(), *EMBEDDED_KEY);
    }

    // ── MultiKeyring: resolve_key_versioned ──────────────────────────────────

    #[test]
    fn versioned_resolution_accepts_matching_version() {
        let key = make_pubkey(0x06);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        // La licencia declara la versión correcta
        assert_eq!(
            keyring.resolve_key_versioned("primary", Some("2026")),
            Some(key)
        );
    }

    #[test]
    fn versioned_resolution_rejects_wrong_version() {
        let key = make_pubkey(0x07);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        // La licencia declara una versión diferente
        assert_eq!(keyring.resolve_key_versioned("primary", Some("2025")), None);
    }

    #[test]
    fn versioned_resolution_rejects_missing_version_when_required() {
        let key = make_pubkey(0x08);
        let keyring = MultiKeyring::new().with_key("primary", key, Some("2026".to_string()));
        // La licencia no declara key_version pero el keyring lo requiere
        assert_eq!(keyring.resolve_key_versioned("primary", None), None);
    }

    #[test]
    fn versioned_resolution_accepts_any_version_when_not_required() {
        let key = make_pubkey(0x09);
        // La entrada no requiere versión específica
        let keyring = MultiKeyring::new().with_key("primary", key, None);
        assert_eq!(
            keyring.resolve_key_versioned("primary", Some("anything")),
            Some(key)
        );
        assert_eq!(keyring.resolve_key_versioned("primary", None), Some(key));
    }

    #[test]
    fn versioned_resolution_rejects_unknown_key_id() {
        let key = make_pubkey(0x0A);
        let keyring = MultiKeyring::new().with_key("primary", key, None);
        assert_eq!(keyring.resolve_key_versioned("unknown", None), None);
    }

    // ── Compatibilidad entre llaves dev/prod (key_id distintos) ─────────────

    #[test]
    fn dev_and_prod_keys_coexist_without_interference() {
        let dev_key = make_pubkey(0x0B);
        let prod_key = make_pubkey(0x0C);
        let keyring = MultiKeyring::new()
            .with_key("primary", dev_key, None)
            .with_key("primary-prod", prod_key, Some("2026".to_string()));

        // dev key resuelve sin restricción de versión
        assert_eq!(keyring.resolve_key("primary"), Some(dev_key));
        // prod key requiere versión correcta
        assert_eq!(
            keyring.resolve_key_versioned("primary-prod", Some("2026")),
            Some(prod_key)
        );
        // prod key rechaza versión incorrecta
        assert_eq!(
            keyring.resolve_key_versioned("primary-prod", Some("dev")),
            None
        );
        // las llaves no se mezclan entre sí
        assert_ne!(dev_key, prod_key);
    }

    // ── EmbeddedKeyring (backward compat) ────────────────────────────────────

    #[test]
    fn embedded_keyring_resolves_primary() {
        let keyring = EmbeddedKeyring;
        assert!(keyring.resolve_key(DEFAULT_KEY_ID).is_some());
        assert!(keyring.resolve_key("unknown").is_none());
    }

    #[test]
    fn embedded_keyring_resolve_key_versioned_ignores_version() {
        // EmbeddedKeyring hereda el default de resolve_key_versioned que ignora versión
        let keyring = EmbeddedKeyring;
        assert!(keyring
            .resolve_key_versioned(DEFAULT_KEY_ID, Some("any"))
            .is_some());
        assert!(keyring
            .resolve_key_versioned(DEFAULT_KEY_ID, None)
            .is_some());
    }

    // ── default_keyring ──────────────────────────────────────────────────────

    #[test]
    fn default_keyring_resolves_primary() {
        let ring = default_keyring();
        assert!(ring.resolve_key(DEFAULT_KEY_ID).is_some());
        assert!(ring.resolve_key("nonexistent").is_none());
    }
}
