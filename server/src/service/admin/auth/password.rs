use std::sync::Arc;

use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use tokio::sync::Semaphore;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::database::manager_credential::{
    MANAGER_ID, MANAGER_SUBJECT, ManagerCredential, ManagerCredentialRepositoryError,
};

pub const PASSWORD_MIN_CODE_POINTS: usize = 15;
pub const PASSWORD_MAX_CODE_POINTS: usize = 128;
pub const ARGON2_MEMORY_KIB: u32 = 65_536;
pub const ARGON2_ITERATIONS: u32 = 3;
pub const ARGON2_LANES: u32 = 4;
pub const ARGON2_OUTPUT_LEN: usize = 32;
pub const ARGON2_SALT_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordPolicyError {
    TooShort,
    TooLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordOperationError {
    Busy,
    IncorrectPassword,
    InvalidVerifier,
    Runtime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialUnavailableReason {
    Storage,
    InvalidIdentity,
    InvalidVerifier,
    InvalidEpoch,
}

pub struct ReadyManagerCredential {
    password_verifier: Zeroizing<String>,
    credential_epoch: Uuid,
}

impl Clone for ReadyManagerCredential {
    fn clone(&self) -> Self {
        Self {
            password_verifier: Zeroizing::new(self.password_verifier.to_string()),
            credential_epoch: self.credential_epoch,
        }
    }
}

impl ReadyManagerCredential {
    pub fn password_verifier(&self) -> Zeroizing<String> {
        Zeroizing::new(self.password_verifier.to_string())
    }

    pub fn credential_epoch(&self) -> Uuid {
        self.credential_epoch
    }
}

#[derive(Clone)]
pub enum ManagerCredentialSnapshot {
    Uninitialized,
    Ready(ReadyManagerCredential),
    Unavailable(CredentialUnavailableReason),
}

impl ManagerCredentialSnapshot {
    pub fn load() -> Self {
        Self::from_repository_result(ManagerCredential::load())
    }

    pub fn from_credential(credential: ManagerCredential) -> Self {
        if credential.manager_id != MANAGER_ID || credential.manager_subject != MANAGER_SUBJECT {
            return Self::Unavailable(CredentialUnavailableReason::InvalidIdentity);
        }
        if validate_verifier(&credential.password_verifier).is_err() {
            return Self::Unavailable(CredentialUnavailableReason::InvalidVerifier);
        }
        let Ok(credential_epoch) = Uuid::parse_str(&credential.credential_epoch) else {
            return Self::Unavailable(CredentialUnavailableReason::InvalidEpoch);
        };

        Self::Ready(ReadyManagerCredential {
            password_verifier: Zeroizing::new(credential.password_verifier),
            credential_epoch,
        })
    }

    fn from_repository_result(
        result: Result<Option<ManagerCredential>, ManagerCredentialRepositoryError>,
    ) -> Self {
        match result {
            Ok(None) => Self::Uninitialized,
            Ok(Some(credential)) => Self::from_credential(credential),
            Err(_) => Self::Unavailable(CredentialUnavailableReason::Storage),
        }
    }
}

pub struct PasswordEngine {
    semaphore: Arc<Semaphore>,
}

impl Default for PasswordEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordEngine {
    pub fn new() -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(1)),
        }
    }

    pub async fn hash(
        &self,
        password: Zeroizing<String>,
    ) -> Result<Zeroizing<String>, PasswordOperationError> {
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| PasswordOperationError::Busy)?;

        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let salt = SaltString::generate(&mut OsRng);
            let verifier = configured_argon2()
                .hash_password(password.as_bytes(), &salt)
                .map_err(|_| PasswordOperationError::Runtime)?
                .to_string();
            validate_verifier(&verifier)?;
            Ok(Zeroizing::new(verifier))
        })
        .await
        .map_err(|_| PasswordOperationError::Runtime)?
    }

    pub async fn verify(
        &self,
        password: Zeroizing<String>,
        verifier: Zeroizing<String>,
    ) -> Result<(), PasswordOperationError> {
        validate_verifier(&verifier)?;
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| PasswordOperationError::Busy)?;

        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let parsed = PasswordHash::new(&verifier)
                .map_err(|_| PasswordOperationError::InvalidVerifier)?;
            configured_argon2()
                .verify_password(password.as_bytes(), &parsed)
                .map_err(|error| match error {
                    argon2::password_hash::Error::Password => {
                        PasswordOperationError::IncorrectPassword
                    }
                    _ => PasswordOperationError::Runtime,
                })
        })
        .await
        .map_err(|_| PasswordOperationError::Runtime)?
    }
}

pub fn normalize_password(input: &str) -> Result<Zeroizing<String>, PasswordPolicyError> {
    let normalized = Zeroizing::new(input.nfc().collect::<String>());
    let code_points = normalized.chars().count();
    if code_points < PASSWORD_MIN_CODE_POINTS {
        return Err(PasswordPolicyError::TooShort);
    }
    if code_points > PASSWORD_MAX_CODE_POINTS {
        return Err(PasswordPolicyError::TooLong);
    }
    Ok(normalized)
}

pub fn validate_verifier(verifier: &str) -> Result<(), PasswordOperationError> {
    let parsed =
        PasswordHash::new(verifier).map_err(|_| PasswordOperationError::InvalidVerifier)?;
    if parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.params.get_decimal("m") != Some(ARGON2_MEMORY_KIB)
        || parsed.params.get_decimal("t") != Some(ARGON2_ITERATIONS)
        || parsed.params.get_decimal("p") != Some(ARGON2_LANES)
        || parsed.hash.as_ref().map(|hash| hash.len()) != Some(ARGON2_OUTPUT_LEN)
    {
        return Err(PasswordOperationError::InvalidVerifier);
    }

    let Some(salt) = parsed.salt else {
        return Err(PasswordOperationError::InvalidVerifier);
    };
    let mut decoded_salt = [0_u8; 64];
    let decoded_salt = salt
        .decode_b64(&mut decoded_salt)
        .map_err(|_| PasswordOperationError::InvalidVerifier)?;
    if decoded_salt.len() != ARGON2_SALT_LEN {
        return Err(PasswordOperationError::InvalidVerifier);
    }
    Ok(())
}

fn configured_argon2() -> Argon2<'static> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_LANES,
        Some(ARGON2_OUTPUT_LEN),
    )
    .expect("fixed Argon2id parameters should be valid");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repeated(character: char, count: usize) -> String {
        std::iter::repeat_n(character, count).collect()
    }

    #[test]
    fn manager_password_policy_uses_nfc_unicode_code_points_without_trimming() {
        assert_eq!(
            normalize_password(&repeated('a', 14)),
            Err(PasswordPolicyError::TooShort)
        );
        assert!(normalize_password(&repeated('界', 15)).is_ok());
        assert!(normalize_password(&repeated('界', 128)).is_ok());
        assert_eq!(
            normalize_password(&repeated('界', 129)),
            Err(PasswordPolicyError::TooLong)
        );

        let decomposed = format!("{}e\u{301}", repeated('a', 14));
        let composed = format!("{}é", repeated('a', 14));
        assert_eq!(
            normalize_password(&decomposed).expect("decomposed value should normalize"),
            normalize_password(&composed).expect("composed value should normalize")
        );
        assert!(normalize_password("               ").is_ok());
        assert!(normalize_password("  password with spaces  ").is_ok());
    }

    #[tokio::test]
    async fn manager_password_engine_hashes_verifies_and_rejects_wrong_password() {
        let engine = PasswordEngine::new();
        let password = normalize_password("correct horse battery staple")
            .expect("password should satisfy policy");
        let verifier = engine
            .hash(password.clone())
            .await
            .expect("hash should succeed");

        validate_verifier(&verifier).expect("generated verifier should match fixed contract");
        engine
            .verify(password, verifier.clone())
            .await
            .expect("correct password should verify");
        assert_eq!(
            engine
                .verify(
                    normalize_password("wrong horse battery staple")
                        .expect("password should satisfy policy"),
                    verifier,
                )
                .await,
            Err(PasswordOperationError::IncorrectPassword)
        );
    }

    #[tokio::test]
    async fn manager_password_engine_rejects_second_operation_without_waiting() {
        let engine = PasswordEngine::new();
        let _occupied = engine
            .semaphore
            .clone()
            .try_acquire_owned()
            .expect("test should acquire only permit");

        assert_eq!(
            engine
                .hash(
                    normalize_password("correct horse battery staple")
                        .expect("password should satisfy policy")
                )
                .await,
            Err(PasswordOperationError::Busy)
        );
    }

    #[test]
    fn manager_credential_snapshot_distinguishes_missing_and_corrupt_records() {
        assert!(matches!(
            ManagerCredentialSnapshot::from_repository_result(Ok(None)),
            ManagerCredentialSnapshot::Uninitialized
        ));
        assert!(matches!(
            ManagerCredentialSnapshot::from_credential(ManagerCredential {
                manager_id: MANAGER_ID,
                manager_subject: MANAGER_SUBJECT.to_string(),
                password_verifier: "not-a-phc".to_string(),
                credential_epoch: Uuid::new_v4().to_string(),
                created_at: 1,
                updated_at: 1,
            }),
            ManagerCredentialSnapshot::Unavailable(CredentialUnavailableReason::InvalidVerifier)
        ));
    }
}
