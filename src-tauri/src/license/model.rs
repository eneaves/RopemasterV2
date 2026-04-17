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
/// Los campos marcados **HYBRID-GROUNDWORK** están presentes y populados pero
/// su enforcement no está activo client-side todavía. Son informativos y permiten
/// al UI mostrar el estado correcto, y a futuro activar enforcement sin cambiar
/// el contrato del struct.
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

    // --- Política híbrida groundwork (parseados, NO enforced client-side hoy) ---
    /// HYBRID-GROUNDWORK: `true` si el operador requiere check-in periódico.
    /// Hoy la licencia se acepta sin check-in. Cuando exista servidor de lease,
    /// una licencia con `lease_required=true` y `max_offline_days` excedido
    /// debe ser bloqueada.
    pub lease_required: bool,

    /// HYBRID-GROUNDWORK: epoch de revocación declarada por el operador.
    /// `None` = sin revocación declarada. Enforcement pendiente de CRL/endpoint.
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
