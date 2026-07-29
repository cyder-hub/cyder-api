use std::fmt;

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use hmac::{Hmac, Mac};
use rand::{Rng, rng};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::Zeroizing;

#[cfg(test)]
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::config::{DownstreamSecretMode, SecretEncryptionConfig, SecretEncryptionKey};
use crate::controller::BaseError;
use crate::database::api_key::{_postgres_model, _sqlite_model};
use crate::database::get_connection;
use crate::database::manager_credential::MANAGER_ID;
use crate::db_execute;
use crate::service::admin::auth::totp::validate_manager_totp_secret;

pub const SECRET_FORMAT_VERSION: i32 = 1;
pub const SECRET_NONCE_LEN: usize = 24;
const PROVIDER_SECRET_INDEX_KEY_LABEL: &[u8] = b"cyder-secret-index-key:v1:provider-api-key";

type HmacSha256 = Hmac<Sha256>;

type StoredSecretTuple = (
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
);

type StoredProviderSecretTuple = (
    i64,
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<String>,
);

type StoredManagerTotpTuple = (
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<i64>,
    Option<i64>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SecretPreparationSummary {
    pub downstream: SecretRotationSummary,
    pub provider: SecretRotationSummary,
    pub manager_totp: ManagerTotpPreparationSummary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ManagerTotpPreparationState {
    #[default]
    Disabled,
    Current,
    Rotated,
    UnavailablePreserved,
}

impl ManagerTotpPreparationState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Current => "current",
            Self::Rotated => "rotated",
            Self::UnavailablePreserved => "unavailable_preserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerTotpPreparationFailure {
    IncompleteFields,
    InvalidFormat,
    UnknownKey,
    DecryptFailed,
    InvalidSecret,
    EncryptFailed,
    ConcurrentChange,
}

impl ManagerTotpPreparationFailure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IncompleteFields => "incomplete_fields",
            Self::InvalidFormat => "invalid_format",
            Self::UnknownKey => "unknown_key",
            Self::DecryptFailed => "decrypt_failed",
            Self::InvalidSecret => "invalid_secret",
            Self::EncryptFailed => "encrypt_failed",
            Self::ConcurrentChange => "concurrent_change",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagerTotpPreparationSummary {
    pub state: ManagerTotpPreparationState,
    pub failure: Option<ManagerTotpPreparationFailure>,
    pub secret_format_version: Option<i32>,
    pub key_fingerprint_short: Option<[u8; 8]>,
}

impl ManagerTotpPreparationSummary {
    pub fn key_fingerprint_short_str(&self) -> Option<&str> {
        self.key_fingerprint_short
            .as_ref()
            .and_then(|value| std::str::from_utf8(value).ok())
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
    ProviderApiKey(i64),
    ManagerTotp(i64),
}

impl SecretDomain {
    fn aad(self) -> Result<Zeroizing<Vec<u8>>, SecretEncryptionError> {
        match self {
            Self::DownstreamApiKey(id) => Ok(Zeroizing::new(
                format!("cyder-secret:v1:downstream-api-key:{id}").into_bytes(),
            )),
            Self::ProviderApiKey(id) => Ok(Zeroizing::new(
                format!("cyder-secret:v1:provider-api-key:{id}").into_bytes(),
            )),
            Self::ManagerTotp(MANAGER_ID) => {
                Ok(Zeroizing::new(b"cyder-secret:v1:manager-totp:0".to_vec()))
            }
            Self::ManagerTotp(_) => Err(SecretEncryptionError::InvalidDomain),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProviderSecretFingerprint(String);

impl ProviderSecretFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ProviderSecretFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProviderSecretFingerprint(<redacted>)")
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
    #[error("stored secret fields are incomplete")]
    IncompleteStoredSecret,
    #[error("secret encryption domain is invalid")]
    InvalidDomain,
}

#[derive(Clone)]
pub struct SecretEncryptionService {
    downstream_mode: DownstreamSecretMode,
    current_key: Option<SecretEncryptionKey>,
    current_fingerprint: Option<KeyFingerprint>,
    #[cfg(test)]
    decrypt_calls: Arc<AtomicUsize>,
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
            #[cfg(test)]
            decrypt_calls: Arc::new(AtomicUsize::new(0)),
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
        #[cfg(test)]
        self.decrypt_calls.fetch_add(1, Ordering::SeqCst);
        let key = self
            .current_key
            .as_ref()
            .ok_or(SecretEncryptionError::KeyUnavailable)?;
        decrypt_with_key(key, domain, encrypted)
    }

    #[cfg(test)]
    pub(crate) fn reset_decrypt_call_count(&self) {
        self.decrypt_calls.store(0, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn decrypt_call_count(&self) -> usize {
        self.decrypt_calls.load(Ordering::SeqCst)
    }

    pub fn provider_secret_fingerprint(
        &self,
        provider_id: i64,
        plaintext: &SensitiveSecret,
    ) -> Result<ProviderSecretFingerprint, SecretEncryptionError> {
        let key = self
            .current_key
            .as_ref()
            .ok_or(SecretEncryptionError::KeyUnavailable)?;
        Ok(provider_secret_fingerprint_with_key(
            key,
            provider_id,
            plaintext,
        ))
    }
}

struct ManagerTotpRotation {
    expected_epoch: String,
    expected_ciphertext: Vec<u8>,
    expected_nonce: Vec<u8>,
    expected_format_version: i32,
    expected_fingerprint: String,
    expected_last_accepted_step: i64,
    expected_enabled_at: i64,
    replacement: EncryptedSecret,
}

struct ManagerTotpInspection {
    summary: ManagerTotpPreparationSummary,
    rotation: Option<ManagerTotpRotation>,
}

fn inspect_manager_totp(
    row: Option<StoredManagerTotpTuple>,
    current_key: &SecretEncryptionKey,
    current_fingerprint: &KeyFingerprint,
    previous_key: Option<&SecretEncryptionKey>,
    previous_fingerprint: Option<&KeyFingerprint>,
) -> ManagerTotpInspection {
    let Some((
        expected_epoch,
        ciphertext,
        nonce,
        format_version,
        fingerprint,
        last_accepted_step,
        enabled_at,
    )) = row
    else {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary::default(),
            rotation: None,
        };
    };

    if [
        ciphertext.is_some(),
        nonce.is_some(),
        format_version.is_some(),
        fingerprint.is_some(),
        last_accepted_step.is_some(),
        enabled_at.is_some(),
    ]
    .iter()
    .all(|present| !present)
    {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary::default(),
            rotation: None,
        };
    }

    let summary_context = ManagerTotpPreparationSummary {
        state: ManagerTotpPreparationState::UnavailablePreserved,
        failure: None,
        secret_format_version: format_version,
        key_fingerprint_short: fingerprint.as_deref().and_then(short_fingerprint_bytes),
    };
    let (
        Some(ciphertext),
        Some(nonce),
        Some(format_version),
        Some(fingerprint),
        Some(last_accepted_step),
        Some(enabled_at),
    ) = (
        ciphertext,
        nonce,
        format_version,
        fingerprint,
        last_accepted_step,
        enabled_at,
    )
    else {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary {
                failure: Some(ManagerTotpPreparationFailure::IncompleteFields),
                ..summary_context
            },
            rotation: None,
        };
    };

    let encrypted = match EncryptedSecret::from_parts(
        ciphertext.clone(),
        nonce.clone(),
        format_version,
        fingerprint.clone(),
    ) {
        Ok(encrypted) => encrypted,
        Err(_) => {
            return ManagerTotpInspection {
                summary: ManagerTotpPreparationSummary {
                    failure: Some(ManagerTotpPreparationFailure::InvalidFormat),
                    ..summary_context
                },
                rotation: None,
            };
        }
    };

    let (plaintext, rotate) = if fingerprint.eq_ignore_ascii_case(current_fingerprint.as_str()) {
        match decrypt_with_key(
            current_key,
            SecretDomain::ManagerTotp(MANAGER_ID),
            &encrypted,
        ) {
            Ok(plaintext) => (plaintext, false),
            Err(_) => {
                return ManagerTotpInspection {
                    summary: ManagerTotpPreparationSummary {
                        failure: Some(ManagerTotpPreparationFailure::DecryptFailed),
                        ..summary_context
                    },
                    rotation: None,
                };
            }
        }
    } else if let Some(previous_key) = previous_key.filter(|_| {
        previous_fingerprint
            .is_some_and(|previous| fingerprint.eq_ignore_ascii_case(previous.as_str()))
    }) {
        match decrypt_with_key(
            previous_key,
            SecretDomain::ManagerTotp(MANAGER_ID),
            &encrypted,
        ) {
            Ok(plaintext) => (plaintext, true),
            Err(_) => {
                return ManagerTotpInspection {
                    summary: ManagerTotpPreparationSummary {
                        failure: Some(ManagerTotpPreparationFailure::DecryptFailed),
                        ..summary_context
                    },
                    rotation: None,
                };
            }
        }
    } else {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary {
                failure: Some(ManagerTotpPreparationFailure::UnknownKey),
                ..summary_context
            },
            rotation: None,
        };
    };

    if validate_manager_totp_secret(&plaintext).is_err() {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary {
                failure: Some(ManagerTotpPreparationFailure::InvalidSecret),
                ..summary_context
            },
            rotation: None,
        };
    }
    if !rotate {
        return ManagerTotpInspection {
            summary: ManagerTotpPreparationSummary {
                state: ManagerTotpPreparationState::Current,
                failure: None,
                ..summary_context
            },
            rotation: None,
        };
    }

    let replacement = match encrypt_with_key(
        current_key,
        SecretDomain::ManagerTotp(MANAGER_ID),
        &plaintext,
    ) {
        Ok(replacement) => replacement,
        Err(_) => {
            return ManagerTotpInspection {
                summary: ManagerTotpPreparationSummary {
                    failure: Some(ManagerTotpPreparationFailure::EncryptFailed),
                    ..summary_context
                },
                rotation: None,
            };
        }
    };
    ManagerTotpInspection {
        summary: ManagerTotpPreparationSummary {
            state: ManagerTotpPreparationState::Rotated,
            failure: None,
            secret_format_version: Some(replacement.format_version()),
            key_fingerprint_short: short_fingerprint_bytes(replacement.key_fingerprint().as_str()),
        },
        rotation: Some(ManagerTotpRotation {
            expected_epoch,
            expected_ciphertext: ciphertext,
            expected_nonce: nonce,
            expected_format_version: format_version,
            expected_fingerprint: fingerprint,
            expected_last_accepted_step: last_accepted_step,
            expected_enabled_at: enabled_at,
            replacement,
        }),
    }
}

fn short_fingerprint_bytes(value: &str) -> Option<[u8; 8]> {
    let prefix = value.as_bytes().get(..8)?;
    if !prefix.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut short = [0_u8; 8];
    for (target, source) in short.iter_mut().zip(prefix.iter().copied()) {
        *target = source.to_ascii_lowercase();
    }
    Some(short)
}

pub fn prepare_secrets_before_startup(
    config: &SecretEncryptionConfig,
) -> Result<SecretPreparationSummary, BaseError> {
    let current_key = config.encryption_key().cloned().ok_or_else(|| {
        BaseError::InternalServerError(Some(
            "Secret preparation failed: current key unavailable".to_string(),
        ))
    })?;
    let previous_key = config.previous_encryption_key();
    let current_fingerprint = key_fingerprint(&current_key);
    let previous_fingerprint = previous_key.as_ref().map(key_fingerprint);
    let conn = &mut get_connection()?;

    let result = db_execute!(conn, {
        conn.transaction::<SecretPreparationSummary, BaseError, _>(|conn| {
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
            let mut downstream = SecretRotationSummary::default();

            for (id, ciphertext, nonce, format_version, fingerprint) in rows {
                let (Some(ciphertext), Some(nonce), Some(format_version), Some(fingerprint)) =
                    (ciphertext, nonce, format_version, fingerprint)
                else {
                    downstream.unavailable_preserved += 1;
                    continue;
                };

                if fingerprint.eq_ignore_ascii_case(current_fingerprint.as_str()) {
                    downstream.current += 1;
                    continue;
                }

                let Some(previous_key) = previous_key.as_ref().filter(|_| {
                    previous_fingerprint
                        .as_ref()
                        .is_some_and(|previous| fingerprint.eq_ignore_ascii_case(previous.as_str()))
                }) else {
                    downstream.unavailable_preserved += 1;
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
                        downstream.unavailable_preserved += 1;
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
                        downstream.unavailable_preserved += 1;
                        continue;
                    }
                };
                let new_encrypted =
                    encrypt_with_key(&current_key, SecretDomain::DownstreamApiKey(id), &plaintext)
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
                downstream.rotated += 1;
            }

            let manager_totp_row = manager_credential::table
                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                .select((
                    manager_credential::dsl::credential_epoch,
                    manager_credential::dsl::totp_secret_ciphertext,
                    manager_credential::dsl::totp_secret_nonce,
                    manager_credential::dsl::totp_secret_format_version,
                    manager_credential::dsl::totp_secret_key_fingerprint,
                    manager_credential::dsl::totp_last_accepted_step,
                    manager_credential::dsl::totp_enabled_at,
                ))
                .first::<StoredManagerTotpTuple>(conn)
                .optional()
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to scan manager TOTP secret: {error}"
                    )))
                })?;
            let mut manager_totp = inspect_manager_totp(
                manager_totp_row,
                &current_key,
                &current_fingerprint,
                previous_key.as_ref(),
                previous_fingerprint.as_ref(),
            );
            if let Some(rotation) = manager_totp.rotation.take() {
                let updated = diesel::update(
                    manager_credential::table.filter(
                        manager_credential::dsl::manager_id
                            .eq(MANAGER_ID)
                            .and(
                                manager_credential::dsl::credential_epoch
                                    .eq(&rotation.expected_epoch),
                            )
                            .and(
                                manager_credential::dsl::totp_secret_ciphertext
                                    .eq(Some(&rotation.expected_ciphertext)),
                            )
                            .and(
                                manager_credential::dsl::totp_secret_nonce
                                    .eq(Some(&rotation.expected_nonce)),
                            )
                            .and(
                                manager_credential::dsl::totp_secret_format_version
                                    .eq(Some(rotation.expected_format_version)),
                            )
                            .and(
                                manager_credential::dsl::totp_secret_key_fingerprint
                                    .eq(Some(&rotation.expected_fingerprint)),
                            )
                            .and(
                                manager_credential::dsl::totp_last_accepted_step
                                    .eq(Some(rotation.expected_last_accepted_step)),
                            )
                            .and(
                                manager_credential::dsl::totp_enabled_at
                                    .eq(Some(rotation.expected_enabled_at)),
                            ),
                    ),
                )
                .set((
                    manager_credential::dsl::totp_secret_ciphertext
                        .eq(Some(rotation.replacement.ciphertext().to_vec())),
                    manager_credential::dsl::totp_secret_nonce
                        .eq(Some(rotation.replacement.nonce().to_vec())),
                    manager_credential::dsl::totp_secret_format_version
                        .eq(Some(rotation.replacement.format_version())),
                    manager_credential::dsl::totp_secret_key_fingerprint.eq(Some(
                        rotation.replacement.key_fingerprint().as_str().to_string(),
                    )),
                ))
                .execute(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to rotate manager TOTP secret: {error}"
                    )))
                })?;
                if updated != 1 {
                    manager_totp.summary.state = ManagerTotpPreparationState::UnavailablePreserved;
                    manager_totp.summary.failure =
                        Some(ManagerTotpPreparationFailure::ConcurrentChange);
                }
            }

            let provider_rows = provider_api_key::table
                .filter(provider_api_key::dsl::deleted_at.is_null())
                .order(provider_api_key::dsl::id.asc())
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::secret_ciphertext,
                    provider_api_key::dsl::secret_nonce,
                    provider_api_key::dsl::secret_format_version,
                    provider_api_key::dsl::secret_key_fingerprint,
                    provider_api_key::dsl::secret_hmac,
                ))
                .load::<StoredProviderSecretTuple>(conn)
                .map_err(|_| {
                    BaseError::DatabaseFatal(Some(
                        "Failed to scan provider API key secrets".to_string(),
                    ))
                })?;
            let mut provider = SecretRotationSummary::default();

            for (id, provider_id, ciphertext, nonce, format_version, fingerprint, stored_hmac) in
                provider_rows
            {
                let (
                    Some(ciphertext),
                    Some(nonce),
                    Some(format_version),
                    Some(fingerprint),
                    Some(stored_hmac),
                ) = (ciphertext, nonce, format_version, fingerprint, stored_hmac)
                else {
                    return Err(provider_secret_startup_error(
                        id,
                        provider_id,
                        "incomplete_fields",
                    ));
                };
                let encrypted = EncryptedSecret::from_parts(
                    ciphertext.clone(),
                    nonce.clone(),
                    format_version,
                    fingerprint.clone(),
                )
                .map_err(|_| provider_secret_startup_error(id, provider_id, "invalid_format"))?;

                let (plaintext, source_key, rotate) =
                    if fingerprint.eq_ignore_ascii_case(current_fingerprint.as_str()) {
                        (
                            decrypt_with_key(
                                &current_key,
                                SecretDomain::ProviderApiKey(id),
                                &encrypted,
                            )
                            .map_err(|_| {
                                provider_secret_startup_error(id, provider_id, "decrypt_failed")
                            })?,
                            &current_key,
                            false,
                        )
                    } else if let Some(previous_key) = previous_key.as_ref().filter(|_| {
                        previous_fingerprint.as_ref().is_some_and(|previous| {
                            fingerprint.eq_ignore_ascii_case(previous.as_str())
                        })
                    }) {
                        (
                            decrypt_with_key(
                                previous_key,
                                SecretDomain::ProviderApiKey(id),
                                &encrypted,
                            )
                            .map_err(|_| {
                                provider_secret_startup_error(id, provider_id, "decrypt_failed")
                            })?,
                            previous_key,
                            true,
                        )
                    } else {
                        return Err(provider_secret_startup_error(
                            id,
                            provider_id,
                            "unknown_key",
                        ));
                    };

                let expected_hmac =
                    provider_secret_fingerprint_with_key(source_key, provider_id, &plaintext);
                if stored_hmac != expected_hmac.as_str() {
                    return Err(provider_secret_startup_error(
                        id,
                        provider_id,
                        "hmac_mismatch",
                    ));
                }

                if !rotate {
                    provider.current += 1;
                    continue;
                }

                let new_encrypted =
                    encrypt_with_key(&current_key, SecretDomain::ProviderApiKey(id), &plaintext)
                        .map_err(|_| {
                            provider_secret_startup_error(id, provider_id, "encrypt_failed")
                        })?;
                let new_hmac =
                    provider_secret_fingerprint_with_key(&current_key, provider_id, &plaintext);
                let updated = diesel::update(
                    provider_api_key::table.filter(
                        provider_api_key::dsl::id
                            .eq(id)
                            .and(provider_api_key::dsl::provider_id.eq(provider_id))
                            .and(provider_api_key::dsl::deleted_at.is_null())
                            .and(provider_api_key::dsl::secret_ciphertext.eq(Some(ciphertext)))
                            .and(provider_api_key::dsl::secret_nonce.eq(Some(nonce)))
                            .and(
                                provider_api_key::dsl::secret_format_version
                                    .eq(Some(format_version)),
                            )
                            .and(
                                provider_api_key::dsl::secret_key_fingerprint.eq(Some(fingerprint)),
                            )
                            .and(provider_api_key::dsl::secret_hmac.eq(Some(stored_hmac))),
                    ),
                )
                .set((
                    provider_api_key::dsl::secret_ciphertext
                        .eq(Some(new_encrypted.ciphertext().to_vec())),
                    provider_api_key::dsl::secret_nonce.eq(Some(new_encrypted.nonce().to_vec())),
                    provider_api_key::dsl::secret_format_version
                        .eq(Some(new_encrypted.format_version())),
                    provider_api_key::dsl::secret_key_fingerprint
                        .eq(Some(new_encrypted.key_fingerprint().as_str().to_string())),
                    provider_api_key::dsl::secret_hmac.eq(Some(new_hmac.as_str().to_string())),
                ))
                .execute(conn)
                .map_err(|_| provider_secret_startup_error(id, provider_id, "write_failed"))?;
                if updated != 1 {
                    return Err(provider_secret_startup_error(
                        id,
                        provider_id,
                        "concurrent_change",
                    ));
                }
                provider.rotated += 1;
            }

            Ok(SecretPreparationSummary {
                downstream,
                provider,
                manager_totp: manager_totp.summary,
            })
        })
    });
    if result.is_ok() {
        config.consume_previous_encryption_key();
    }
    result
}

fn provider_secret_startup_error(id: i64, provider_id: i64, reason: &'static str) -> BaseError {
    BaseError::InternalServerError(Some(format!(
        "Provider secret preparation failed for key_id={id}, provider_id={provider_id}, reason={reason}"
    )))
}

fn provider_secret_fingerprint_with_key(
    key: &SecretEncryptionKey,
    provider_id: i64,
    plaintext: &SensitiveSecret,
) -> ProviderSecretFingerprint {
    let index_key = hmac_sha256(key.as_bytes(), PROVIDER_SECRET_INDEX_KEY_LABEL);
    let mut input = Zeroizing::new(Vec::with_capacity(
        std::mem::size_of::<i64>() + plaintext.expose().len(),
    ));
    input.extend_from_slice(&provider_id.to_be_bytes());
    input.extend_from_slice(plaintext.expose().as_bytes());
    ProviderSecretFingerprint(hex_lower(&hmac_sha256(&index_key, &input)))
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
    let aad = domain.aad()?;
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
    let aad = domain.aad()?;
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

fn hmac_sha256(key: &[u8], input: &[u8]) -> [u8; 32] {
    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(input);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod startup_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const KEY_B: &str = "101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f";

    fn config_for_key(key: &str) -> SecretEncryptionConfig {
        serde_yaml::from_str(&format!(
            "downstream_mode: recoverable\nencryption_key: '{key}'\n"
        ))
        .expect("recoverable secret config should parse")
    }

    fn recoverable_config() -> SecretEncryptionConfig {
        config_for_key(KEY_A)
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
    fn provider_domain_uses_fixed_aad_and_cannot_cross_decrypt() {
        assert_eq!(
            SecretDomain::ProviderApiKey(41)
                .aad()
                .expect("provider domain should be valid")
                .as_slice(),
            b"cyder-secret:v1:provider-api-key:41"
        );

        let service = SecretEncryptionService::from_config(&recoverable_config());
        let plaintext = SensitiveSecret::new("provider-secret-value".to_string());
        let encrypted = service
            .encrypt_current(SecretDomain::ProviderApiKey(41), &plaintext)
            .expect("provider secret should encrypt");

        assert_eq!(
            service
                .decrypt_current(SecretDomain::ProviderApiKey(41), &encrypted)
                .expect("matching provider domain should decrypt")
                .expose(),
            plaintext.expose()
        );
        assert!(matches!(
            service.decrypt_current(SecretDomain::ProviderApiKey(42), &encrypted),
            Err(SecretEncryptionError::DecryptFailed)
        ));
        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(41), &encrypted),
            Err(SecretEncryptionError::DecryptFailed)
        ));
    }

    #[test]
    fn manager_totp_domain_uses_fixed_aad_and_rejects_cross_domain_or_manager_id() {
        assert_eq!(
            SecretDomain::ManagerTotp(MANAGER_ID)
                .aad()
                .expect("singleton manager TOTP domain should be valid")
                .as_slice(),
            b"cyder-secret:v1:manager-totp:0"
        );
        assert_eq!(
            SecretDomain::ManagerTotp(1).aad(),
            Err(SecretEncryptionError::InvalidDomain)
        );

        let service = SecretEncryptionService::from_config(&recoverable_config());
        let plaintext = SensitiveSecret::new("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP".to_string());
        let encrypted = service
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &plaintext)
            .expect("manager TOTP secret should encrypt");
        assert_eq!(
            service
                .decrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &encrypted)
                .expect("matching manager TOTP domain should decrypt")
                .expose(),
            plaintext.expose()
        );
        assert!(matches!(
            service.decrypt_current(SecretDomain::DownstreamApiKey(MANAGER_ID), &encrypted),
            Err(SecretEncryptionError::DecryptFailed)
        ));
        assert!(matches!(
            service.decrypt_current(SecretDomain::ProviderApiKey(MANAGER_ID), &encrypted),
            Err(SecretEncryptionError::DecryptFailed)
        ));
        assert!(matches!(
            service.decrypt_current(SecretDomain::ManagerTotp(1), &encrypted),
            Err(SecretEncryptionError::InvalidDomain)
        ));
    }

    #[test]
    fn provider_secret_fingerprint_is_keyed_stable_and_domain_scoped() {
        let service_a = SecretEncryptionService::from_config(&config_for_key(KEY_A));
        let service_b = SecretEncryptionService::from_config(&config_for_key(KEY_B));
        let exact = SensitiveSecret::new(" provider-secret ".to_string());
        let trimmed = SensitiveSecret::new("provider-secret".to_string());

        let first = service_a
            .provider_secret_fingerprint(7, &exact)
            .expect("provider fingerprint should derive");
        let repeated = service_a
            .provider_secret_fingerprint(7, &exact)
            .expect("provider fingerprint should be stable");
        let different_provider = service_a
            .provider_secret_fingerprint(8, &exact)
            .expect("provider id should scope fingerprint");
        let different_bytes = service_a
            .provider_secret_fingerprint(7, &trimmed)
            .expect("exact UTF-8 bytes should be preserved");
        let different_key = service_b
            .provider_secret_fingerprint(7, &exact)
            .expect("master key should scope fingerprint");

        assert_eq!(first, repeated);
        assert_eq!(first.as_str().len(), 64);
        assert!(
            first
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_ne!(first, different_provider);
        assert_ne!(first, different_bytes);
        assert_ne!(first, different_key);
        assert!(!format!("{first:?}").contains(exact.expose()));
        assert!(!format!("{first:?}").contains(first.as_str()));
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
