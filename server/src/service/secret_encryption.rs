use std::fmt;

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::{Rng, rng};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::config::{DownstreamSecretMode, SecretEncryptionConfig, SecretEncryptionKey};
use crate::controller::BaseError;
use crate::database::api_key::{_postgres_model, _sqlite_model};
use crate::database::get_connection;
use crate::db_execute;

pub const SECRET_FORMAT_VERSION: i32 = 1;
pub const SECRET_NONCE_LEN: usize = 24;

type StoredSecretTuple = (
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecretRotationSummary {
    pub current: usize,
    pub rotated: usize,
    pub unavailable_preserved: usize,
}

impl SecretRotationSummary {
    pub fn total(&self) -> usize {
        self.current + self.rotated + self.unavailable_preserved
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct KeyFingerprint(String);

impl KeyFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}

impl fmt::Debug for KeyFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KeyFingerprint(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretDomain {
    DownstreamApiKey(i64),
}

impl SecretDomain {
    fn aad(self) -> Zeroizing<Vec<u8>> {
        match self {
            Self::DownstreamApiKey(id) => {
                Zeroizing::new(format!("cyder-secret:v1:downstream-api-key:{id}").into_bytes())
            }
        }
    }
}

pub struct SensitiveSecret(Zeroizing<String>);

impl SensitiveSecret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }

    pub fn to_unprotected_string(&self) -> String {
        self.0.to_string()
    }
}

impl fmt::Debug for SensitiveSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SensitiveSecret(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    ciphertext: Vec<u8>,
    nonce: [u8; SECRET_NONCE_LEN],
    format_version: i32,
    key_fingerprint: KeyFingerprint,
}

impl EncryptedSecret {
    pub fn from_parts(
        ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        format_version: i32,
        key_fingerprint: String,
    ) -> Result<Self, SecretEncryptionError> {
        let nonce = nonce
            .try_into()
            .map_err(|_| SecretEncryptionError::InvalidNonce)?;
        if format_version != SECRET_FORMAT_VERSION {
            return Err(SecretEncryptionError::UnsupportedFormat);
        }
        if key_fingerprint.len() != 64
            || !key_fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SecretEncryptionError::InvalidFingerprint);
        }
        Ok(Self {
            ciphertext,
            nonce,
            format_version,
            key_fingerprint: KeyFingerprint(key_fingerprint.to_ascii_lowercase()),
        })
    }

    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn nonce(&self) -> &[u8; SECRET_NONCE_LEN] {
        &self.nonce
    }

    pub fn format_version(&self) -> i32 {
        self.format_version
    }

    pub fn key_fingerprint(&self) -> &KeyFingerprint {
        &self.key_fingerprint
    }
}

impl fmt::Debug for EncryptedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EncryptedSecret(<redacted>)")
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum SecretEncryptionError {
    #[error("secret encryption key is not configured")]
    KeyUnavailable,
    #[error("secret encryption format is unsupported")]
    UnsupportedFormat,
    #[error("secret encryption nonce is invalid")]
    InvalidNonce,
    #[error("secret encryption key fingerprint is invalid")]
    InvalidFingerprint,
    #[error("secret encryption key does not match the stored fingerprint")]
    KeyMismatch,
    #[error("failed to encrypt secret")]
    EncryptFailed,
    #[error("failed to decrypt secret")]
    DecryptFailed,
    #[error("decrypted secret is not valid UTF-8")]
    InvalidPlaintext,
}

#[derive(Clone)]
pub struct SecretEncryptionService {
    downstream_mode: DownstreamSecretMode,
    current_key: Option<SecretEncryptionKey>,
    current_fingerprint: Option<KeyFingerprint>,
}

impl fmt::Debug for SecretEncryptionService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretEncryptionService(<redacted>)")
    }
}

impl SecretEncryptionService {
    pub fn from_config(config: &SecretEncryptionConfig) -> Self {
        let current_key = config.encryption_key().cloned();
        let current_fingerprint = current_key.as_ref().map(key_fingerprint);
        Self {
            downstream_mode: config.downstream_mode,
            current_key,
            current_fingerprint,
        }
    }

    pub fn downstream_mode(&self) -> DownstreamSecretMode {
        self.downstream_mode
    }

    pub fn has_current_key(&self) -> bool {
        self.current_key.is_some()
    }

    pub fn current_fingerprint(&self) -> Option<&KeyFingerprint> {
        self.current_fingerprint.as_ref()
    }

    pub fn can_reveal(&self, secret_tuple_complete: bool, fingerprint: Option<&str>) -> bool {
        self.downstream_mode == DownstreamSecretMode::Recoverable
            && secret_tuple_complete
            && self.current_fingerprint.as_ref().is_some_and(|current| {
                fingerprint.is_some_and(|stored| stored.eq_ignore_ascii_case(current.as_str()))
            })
    }

    pub fn encrypt_current(
        &self,
        domain: SecretDomain,
        plaintext: &SensitiveSecret,
    ) -> Result<EncryptedSecret, SecretEncryptionError> {
        let key = self
            .current_key
            .as_ref()
            .ok_or(SecretEncryptionError::KeyUnavailable)?;
        encrypt_with_key(key, domain, plaintext)
    }

    pub fn decrypt_current(
        &self,
        domain: SecretDomain,
        encrypted: &EncryptedSecret,
    ) -> Result<SensitiveSecret, SecretEncryptionError> {
        let key = self
            .current_key
            .as_ref()
            .ok_or(SecretEncryptionError::KeyUnavailable)?;
        decrypt_with_key(key, domain, encrypted)
    }
}

pub fn rotate_downstream_secrets_before_startup(
    config: &SecretEncryptionConfig,
) -> Result<SecretRotationSummary, BaseError> {
    let current_key = config.encryption_key().cloned();
    let previous_key = config.take_previous_encryption_key();
    let current_fingerprint = current_key.as_ref().map(key_fingerprint);
    let previous_fingerprint = previous_key.as_ref().map(key_fingerprint);
    let conn = &mut get_connection()?;

    db_execute!(conn, {
        conn.transaction::<SecretRotationSummary, BaseError, _>(|conn| {
            let rows = api_key::table
                .filter(api_key::dsl::secret_ciphertext.is_not_null())
                .order(api_key::dsl::id.asc())
                .select((
                    api_key::dsl::id,
                    api_key::dsl::secret_ciphertext,
                    api_key::dsl::secret_nonce,
                    api_key::dsl::secret_format_version,
                    api_key::dsl::secret_key_fingerprint,
                ))
                .load::<StoredSecretTuple>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to scan downstream api key secrets: {error}"
                    )))
                })?;
            let mut summary = SecretRotationSummary::default();

            for (id, ciphertext, nonce, format_version, fingerprint) in rows {
                let (Some(ciphertext), Some(nonce), Some(format_version), Some(fingerprint)) =
                    (ciphertext, nonce, format_version, fingerprint)
                else {
                    summary.unavailable_preserved += 1;
                    continue;
                };

                if current_fingerprint
                    .as_ref()
                    .is_some_and(|current| fingerprint.eq_ignore_ascii_case(current.as_str()))
                {
                    summary.current += 1;
                    continue;
                }

                let Some(previous_key) = previous_key.as_ref().filter(|_| {
                    previous_fingerprint
                        .as_ref()
                        .is_some_and(|previous| fingerprint.eq_ignore_ascii_case(previous.as_str()))
                }) else {
                    summary.unavailable_preserved += 1;
                    continue;
                };
                let Some(current_key) = current_key.as_ref() else {
                    summary.unavailable_preserved += 1;
                    continue;
                };

                let old_encrypted = match EncryptedSecret::from_parts(
                    ciphertext.clone(),
                    nonce.clone(),
                    format_version,
                    fingerprint.clone(),
                ) {
                    Ok(encrypted) => encrypted,
                    Err(_) => {
                        summary.unavailable_preserved += 1;
                        continue;
                    }
                };
                let plaintext = match decrypt_with_key(
                    previous_key,
                    SecretDomain::DownstreamApiKey(id),
                    &old_encrypted,
                ) {
                    Ok(plaintext) => plaintext,
                    Err(_) => {
                        summary.unavailable_preserved += 1;
                        continue;
                    }
                };
                let new_encrypted =
                    encrypt_with_key(current_key, SecretDomain::DownstreamApiKey(id), &plaintext)
                        .map_err(|_| {
                        BaseError::InternalServerError(Some(
                            "Failed to re-encrypt downstream api key secret".to_string(),
                        ))
                    })?;

                let updated = diesel::update(
                    api_key::table.filter(
                        api_key::dsl::id
                            .eq(id)
                            .and(api_key::dsl::secret_ciphertext.eq(Some(ciphertext)))
                            .and(api_key::dsl::secret_nonce.eq(Some(nonce)))
                            .and(api_key::dsl::secret_format_version.eq(Some(format_version)))
                            .and(api_key::dsl::secret_key_fingerprint.eq(Some(fingerprint))),
                    ),
                )
                .set((
                    api_key::dsl::secret_ciphertext.eq(Some(new_encrypted.ciphertext().to_vec())),
                    api_key::dsl::secret_nonce.eq(Some(new_encrypted.nonce().to_vec())),
                    api_key::dsl::secret_format_version.eq(Some(new_encrypted.format_version())),
                    api_key::dsl::secret_key_fingerprint
                        .eq(Some(new_encrypted.key_fingerprint().as_str().to_string())),
                ))
                .execute(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to rotate downstream api key secret {id}: {error}"
                    )))
                })?;
                if updated != 1 {
                    return Err(BaseError::DatabaseFatal(Some(format!(
                        "Downstream api key secret {id} changed during startup rotation"
                    ))));
                }
                summary.rotated += 1;
            }

            Ok(summary)
        })
    })
}

pub(crate) fn key_fingerprint(key: &SecretEncryptionKey) -> KeyFingerprint {
    let digest = Sha256::digest(key.as_bytes());
    KeyFingerprint(hex_lower(&digest))
}

pub(crate) fn encrypt_with_key(
    key: &SecretEncryptionKey,
    domain: SecretDomain,
    plaintext: &SensitiveSecret,
) -> Result<EncryptedSecret, SecretEncryptionError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| SecretEncryptionError::EncryptFailed)?;
    let mut nonce = [0_u8; SECRET_NONCE_LEN];
    rng().fill(&mut nonce);
    let aad = domain.aad();
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext.expose().as_bytes(),
                aad: &aad,
            },
        )
        .map_err(|_| SecretEncryptionError::EncryptFailed)?;
    Ok(EncryptedSecret {
        ciphertext,
        nonce,
        format_version: SECRET_FORMAT_VERSION,
        key_fingerprint: key_fingerprint(key),
    })
}

pub(crate) fn decrypt_with_key(
    key: &SecretEncryptionKey,
    domain: SecretDomain,
    encrypted: &EncryptedSecret,
) -> Result<SensitiveSecret, SecretEncryptionError> {
    if encrypted.format_version != SECRET_FORMAT_VERSION {
        return Err(SecretEncryptionError::UnsupportedFormat);
    }
    if encrypted.key_fingerprint != key_fingerprint(key) {
        return Err(SecretEncryptionError::KeyMismatch);
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_bytes())
        .map_err(|_| SecretEncryptionError::DecryptFailed)?;
    let aad = domain.aad();
    let plaintext = cipher
        .decrypt(
            &XNonce::from(encrypted.nonce),
            Payload {
                msg: &encrypted.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| SecretEncryptionError::DecryptFailed)?;
    let plaintext =
        String::from_utf8(plaintext).map_err(|_| SecretEncryptionError::InvalidPlaintext)?;
    Ok(SensitiveSecret::new(plaintext))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod startup_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn recoverable_config() -> SecretEncryptionConfig {
        serde_yaml::from_str(&format!(
            "downstream_mode: recoverable\nencryption_key: '{KEY_A}'\n"
        ))
        .expect("recoverable secret config should parse")
    }

    #[test]
    fn encrypt_decrypt_round_trip_uses_random_nonce_and_domain_aad() {
        let service = SecretEncryptionService::from_config(&recoverable_config());
        let plaintext = SensitiveSecret::new("cyder-secret-value".to_string());

        let first = service
            .encrypt_current(SecretDomain::DownstreamApiKey(41), &plaintext)
            .expect("first encryption should succeed");
        let second = service
            .encrypt_current(SecretDomain::DownstreamApiKey(41), &plaintext)
            .expect("second encryption should succeed");

        assert_ne!(first.nonce(), second.nonce());
        assert_ne!(first.ciphertext(), second.ciphertext());
        let decrypted = service
            .decrypt_current(SecretDomain::DownstreamApiKey(41), &first)
            .expect("matching domain should decrypt");
        assert_eq!(decrypted.expose(), plaintext.expose());
        assert_eq!(first.format_version(), SECRET_FORMAT_VERSION);
        assert_eq!(first.key_fingerprint().as_str().len(), 64);
        assert_eq!(first.key_fingerprint().short().len(), 8);

        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(42), &first),
            Err(SecretEncryptionError::DecryptFailed)
        ));
    }

    #[test]
    fn tampering_ciphertext_nonce_or_fingerprint_is_rejected() {
        let service = SecretEncryptionService::from_config(&recoverable_config());
        let plaintext = SensitiveSecret::new("cyder-secret-value".to_string());
        let encrypted = service
            .encrypt_current(SecretDomain::DownstreamApiKey(7), &plaintext)
            .expect("encryption should succeed");

        let mut ciphertext = encrypted.clone();
        ciphertext.ciphertext[0] ^= 1;
        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(7), &ciphertext),
            Err(SecretEncryptionError::DecryptFailed)
        ));

        let mut nonce = encrypted.clone();
        nonce.nonce[0] ^= 1;
        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(7), &nonce),
            Err(SecretEncryptionError::DecryptFailed)
        ));

        let mut fingerprint = encrypted;
        let replacement = if fingerprint.key_fingerprint.0.starts_with('0') {
            "f"
        } else {
            "0"
        };
        fingerprint
            .key_fingerprint
            .0
            .replace_range(0..1, replacement);
        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(7), &fingerprint),
            Err(SecretEncryptionError::KeyMismatch)
        ));
    }

    #[test]
    fn debug_output_redacts_keys_plaintext_and_encrypted_material() {
        let config = recoverable_config();
        let service = SecretEncryptionService::from_config(&config);
        let plaintext = SensitiveSecret::new("cyder-never-log-this".to_string());
        let encrypted = service
            .encrypt_current(SecretDomain::DownstreamApiKey(9), &plaintext)
            .expect("encryption should succeed");
        let config_debug = format!("{config:?}");
        let service_debug = format!("{service:?}");
        let plaintext_debug = format!("{plaintext:?}");
        let encrypted_debug = format!("{encrypted:?}");

        for output in [
            config_debug,
            service_debug,
            plaintext_debug,
            encrypted_debug,
        ] {
            assert!(!output.contains(KEY_A));
            assert!(!output.contains("cyder-never-log-this"));
            assert!(!output.contains(encrypted.key_fingerprint().as_str()));
        }
    }

    #[test]
    fn one_time_without_key_cannot_encrypt() {
        let service = SecretEncryptionService::from_config(&SecretEncryptionConfig::default());
        assert_eq!(service.downstream_mode(), DownstreamSecretMode::OneTime);
        assert!(!service.has_current_key());
        assert_eq!(
            service.encrypt_current(
                SecretDomain::DownstreamApiKey(1),
                &SensitiveSecret::new("secret".to_string())
            ),
            Err(SecretEncryptionError::KeyUnavailable)
        );
    }
}
