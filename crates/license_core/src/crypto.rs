use ed25519_dalek::{PublicKey, Signature, Verifier};

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("invalid signature bytes")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

pub fn verify_signature(
    public_key: &PublicKey,
    payload_bytes: &[u8],
    signature_bytes: &[u8; 64],
) -> Result<(), SignatureError> {
    let signature = Signature::from_bytes(signature_bytes).map_err(|_| SignatureError::InvalidSignature)?;
    public_key
        .verify(payload_bytes, &signature)
        .map_err(|_| SignatureError::VerificationFailed)
}
