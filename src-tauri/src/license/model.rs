use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseFormatKind {
    ModernLicgen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMatch {
    Current,
    LegacyCompat,
    Mismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum NormalizedFailureReason {
    Parse,
    InvalidFormat,
    InvalidSignature,
    UnknownKeyId,
    MissingKeyId,
    AppIdMismatch,
    UnsupportedVersion,
    DeviceMismatch,
    NotYetValid,
    Expired,
}

/// Representación normalizada de una licencia evaluada.
///
/// Los campos híbridos permanecen en el contrato normalizado para diagnóstico,
/// pero una licencia que los declare debe ser rechazada por la verificación
/// endurecida antes de llegar a estado activo.
#[derive(Debug, Clone, Serialize)]
pub struct NormalizedLicense {
    // --- Campos de identidad ---
    pub format: LicenseFormatKind,
    pub format_version: u16,
    pub app_id: String,
    pub license_id: String,
    pub plan: Option<String>,
    pub customer_name: Option<String>,

    // --- Campos de firma y clave ---
    pub signature_valid: bool,
    pub key_id: Option<String>,
    pub key_version: Option<String>,

    // --- Ventana temporal ---
    pub issued_at: i64,
    pub not_before: i64,
    pub not_after: i64,
    pub max_clock_skew: i64,

    // --- Política offline (enforced hoy) ---
    /// Máximo de días que la licencia puede usarse sin contacto al servidor.
    /// Validado al parsear (> 0). En licencias puramente offline, `expires_at`
    /// es el límite real; `max_offline_days` cobra relevancia solo en modo híbrido.
    pub max_offline_days: u16,

    // --- Política híbrida no soportada ---
    /// `true` si la licencia declaraba check-in periódico. El runtime endurecido
    /// rechaza estas licencias como no soportadas.
    pub lease_required: bool,

    /// Epoch de revocación declarada por el operador. El runtime endurecido
    /// rechaza estas licencias como no soportadas.
    pub revocation_epoch: Option<u64>,

    /// Número de fingerprints en la lista `allowed_fingerprints`.
    /// `0` = sin restricción de fingerprint (cualquier hash válido acepta).
    /// `> 0` = lista activa; enforced desde Fase 5 en el lado cliente también.
    pub allowed_fingerprints_count: usize,

    // --- Binding y dispositivo ---
    pub device_hash_hex: String,
    pub installation_id: Option<String>,
    pub installation_pubkey: Option<String>,
    pub binding: BindingMatch,

    // --- Diagnóstico del blob ---
    pub blob_len: usize,
    pub blob_sha256: String,
    pub failure_reason: Option<NormalizedFailureReason>,
}
