use std::collections::HashSet;
use std::fmt;

use rand::{Rng, rng};
use thiserror::Error;
use totp_rs::{Algorithm, Secret, TOTP};
use zeroize::Zeroizing;

use crate::service::secret_encryption::SensitiveSecret;

pub(crate) const MANAGER_TOTP_DIGITS: usize = 6;
pub(crate) const MANAGER_TOTP_PERIOD_SEC: i64 = 30;
pub(crate) const MANAGER_TOTP_SECRET_BYTES: usize = 20;
const MANAGER_TOTP_WINDOW_STEPS: i64 = 1;
const MANAGER_TOTP_ISSUER: &str = "Cyder";
const MANAGER_TOTP_ACCOUNT: &str = "manager";
const RECOVERY_CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub(crate) const MANAGER_TOTP_RECOVERY_CODE_COUNT: usize = 10;
const RECOVERY_CODE_ID_CHARS: usize = 4;
const RECOVERY_CODE_SECRET_CHARS: usize = 16;
const RECOVERY_CODE_NORMALIZED_CHARS: usize = RECOVERY_CODE_ID_CHARS + RECOVERY_CODE_SECRET_CHARS;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagerTotpPrimitiveError {
    #[error("manager TOTP secret is invalid")]
    InvalidSecret,
    #[error("manager TOTP code format is invalid")]
    InvalidCodeFormat,
    #[error("manager TOTP timestamp is invalid")]
    InvalidTimestamp,
}

pub(crate) struct ManagerTotpProvisioning {
    manual_secret: SensitiveSecret,
    otpauth_uri: SensitiveSecret,
}

impl ManagerTotpProvisioning {
    pub(crate) fn manual_secret(&self) -> &SensitiveSecret {
        &self.manual_secret
    }

    pub(crate) fn otpauth_uri(&self) -> &SensitiveSecret {
        &self.otpauth_uri
    }
}

impl fmt::Debug for ManagerTotpProvisioning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ManagerTotpProvisioning(<redacted>)")
    }
}

pub(crate) struct GeneratedRecoveryCode {
    code_id: String,
    normalized: Zeroizing<String>,
    display: SensitiveSecret,
}

impl GeneratedRecoveryCode {
    pub(crate) fn code_id(&self) -> &str {
        &self.code_id
    }

    pub(crate) fn normalized_for_hash(&self) -> Zeroizing<String> {
        Zeroizing::new(self.normalized.to_string())
    }

    pub(crate) fn display(&self) -> &SensitiveSecret {
        &self.display
    }
}

impl fmt::Debug for GeneratedRecoveryCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GeneratedRecoveryCode(<redacted>)")
    }
}

pub(crate) fn generate_manager_totp_provisioning()
-> Result<ManagerTotpProvisioning, ManagerTotpPrimitiveError> {
    let mut raw_secret = Zeroizing::new(vec![0_u8; MANAGER_TOTP_SECRET_BYTES]);
    rng().fill(raw_secret.as_mut_slice());
    let totp = build_totp_from_raw(std::mem::take(&mut *raw_secret))?;
    let manual_secret = SensitiveSecret::new(totp.get_secret_base32());
    let otpauth_uri = SensitiveSecret::new(format!(
        "otpauth://totp/{MANAGER_TOTP_ISSUER}:{MANAGER_TOTP_ACCOUNT}?secret={}&issuer={MANAGER_TOTP_ISSUER}&algorithm=SHA1&digits={MANAGER_TOTP_DIGITS}&period={MANAGER_TOTP_PERIOD_SEC}",
        manual_secret.expose()
    ));

    Ok(ManagerTotpProvisioning {
        manual_secret,
        otpauth_uri,
    })
}

pub(crate) fn validate_manager_totp_secret(
    secret: &SensitiveSecret,
) -> Result<(), ManagerTotpPrimitiveError> {
    build_totp_from_base32(secret).map(drop)
}

pub(crate) fn validate_manager_totp_code(code: &str) -> Result<(), ManagerTotpPrimitiveError> {
    if code.len() != MANAGER_TOTP_DIGITS || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ManagerTotpPrimitiveError::InvalidCodeFormat);
    }
    Ok(())
}

pub(crate) fn match_manager_totp_step(
    secret: &SensitiveSecret,
    code: &str,
    unix_seconds: i64,
) -> Result<Option<i64>, ManagerTotpPrimitiveError> {
    validate_manager_totp_code(code)?;
    if unix_seconds < 0 {
        return Err(ManagerTotpPrimitiveError::InvalidTimestamp);
    }
    let current_step = unix_seconds / MANAGER_TOTP_PERIOD_SEC;
    let totp = build_totp_from_base32(secret)?;

    select_max_matching_step(current_step, |candidate_step| {
        let candidate_seconds = u64::try_from(candidate_step)
            .ok()
            .and_then(|step| step.checked_mul(MANAGER_TOTP_PERIOD_SEC as u64));
        candidate_seconds.is_some_and(|timestamp| totp.check(code, timestamp))
    })
}

pub(crate) fn generate_manager_totp_recovery_codes() -> Vec<GeneratedRecoveryCode> {
    let mut code_ids = HashSet::with_capacity(MANAGER_TOTP_RECOVERY_CODE_COUNT);
    let mut codes = Vec::with_capacity(MANAGER_TOTP_RECOVERY_CODE_COUNT);
    while codes.len() < MANAGER_TOTP_RECOVERY_CODE_COUNT {
        let mut normalized = Zeroizing::new(String::with_capacity(RECOVERY_CODE_NORMALIZED_CHARS));
        for _ in 0..RECOVERY_CODE_NORMALIZED_CHARS {
            normalized.push(RECOVERY_CODE_ALPHABET[rng().random_range(0..32)] as char);
        }
        let code_id = normalized[..RECOVERY_CODE_ID_CHARS].to_string();
        if !code_ids.insert(code_id.clone()) {
            continue;
        }
        let mut display = String::with_capacity(RECOVERY_CODE_NORMALIZED_CHARS + 4);
        for (index, character) in normalized.chars().enumerate() {
            if index > 0 && index % 4 == 0 {
                display.push('-');
            }
            display.push(character);
        }
        codes.push(GeneratedRecoveryCode {
            code_id,
            normalized,
            display: SensitiveSecret::new(display),
        });
    }
    codes
}

pub(crate) fn normalize_manager_totp_recovery_code(
    input: &str,
) -> Result<(String, Zeroizing<String>), ManagerTotpPrimitiveError> {
    let normalized = Zeroizing::new(
        input
            .chars()
            .filter(|character| *character != '-')
            .flat_map(char::to_uppercase)
            .collect::<String>(),
    );
    if normalized.len() != RECOVERY_CODE_NORMALIZED_CHARS
        || !normalized
            .bytes()
            .all(|byte| RECOVERY_CODE_ALPHABET.contains(&byte))
    {
        return Err(ManagerTotpPrimitiveError::InvalidSecret);
    }
    Ok((normalized[..RECOVERY_CODE_ID_CHARS].to_string(), normalized))
}

fn select_max_matching_step(
    current_step: i64,
    mut matches: impl FnMut(i64) -> bool,
) -> Result<Option<i64>, ManagerTotpPrimitiveError> {
    let first_step = current_step.saturating_sub(MANAGER_TOTP_WINDOW_STEPS);
    let last_step = current_step
        .checked_add(MANAGER_TOTP_WINDOW_STEPS)
        .ok_or(ManagerTotpPrimitiveError::InvalidTimestamp)?;
    let mut matched_step = None;
    for candidate_step in first_step..=last_step {
        if matches(candidate_step) {
            matched_step = Some(candidate_step);
        }
    }
    Ok(matched_step)
}

fn build_totp_from_base32(secret: &SensitiveSecret) -> Result<TOTP, ManagerTotpPrimitiveError> {
    let encoded = Secret::Encoded(secret.to_unprotected_string());
    let mut raw = Zeroizing::new(
        encoded
            .to_bytes()
            .map_err(|_| ManagerTotpPrimitiveError::InvalidSecret)?,
    );
    if raw.len() != MANAGER_TOTP_SECRET_BYTES {
        return Err(ManagerTotpPrimitiveError::InvalidSecret);
    }
    build_totp_from_raw(std::mem::take(&mut *raw))
}

fn build_totp_from_raw(raw_secret: Vec<u8>) -> Result<TOTP, ManagerTotpPrimitiveError> {
    TOTP::new(
        Algorithm::SHA1,
        MANAGER_TOTP_DIGITS,
        0,
        MANAGER_TOTP_PERIOD_SEC as u64,
        raw_secret,
        Some(MANAGER_TOTP_ISSUER.to_string()),
        MANAGER_TOTP_ACCOUNT.to_string(),
    )
    .map_err(|_| ManagerTotpPrimitiveError::InvalidSecret)
}

#[cfg(test)]
pub(crate) fn generate_manager_totp_code(
    secret: &SensitiveSecret,
    unix_seconds: i64,
) -> Result<Zeroizing<String>, ManagerTotpPrimitiveError> {
    let timestamp =
        u64::try_from(unix_seconds).map_err(|_| ManagerTotpPrimitiveError::InvalidTimestamp)?;
    Ok(Zeroizing::new(
        build_totp_from_base32(secret)?.generate(timestamp),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SHA1_SECRET_BASE32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn manager_totp_uses_rfc6238_sha1_six_digit_vectors() {
        let secret = SensitiveSecret::new(RFC_SHA1_SECRET_BASE32.to_string());
        for (timestamp, expected) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_111_111_111, "050471"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
            (20_000_000_000, "353130"),
        ] {
            assert_eq!(
                generate_manager_totp_code(&secret, timestamp)
                    .expect("RFC vector should generate")
                    .as_str(),
                expected
            );
        }
    }

    #[test]
    fn manager_totp_matches_only_plus_or_minus_one_step() {
        let secret = SensitiveSecret::new(RFC_SHA1_SECRET_BASE32.to_string());
        let center_timestamp = 1_234_567_890;
        let center_step = center_timestamp / MANAGER_TOTP_PERIOD_SEC;

        for candidate_step in (center_step - 2)..=(center_step + 2) {
            let code =
                generate_manager_totp_code(&secret, candidate_step * MANAGER_TOTP_PERIOD_SEC)
                    .expect("candidate code should generate");
            let matched = match_manager_totp_step(&secret, code.as_str(), center_timestamp)
                .expect("candidate should evaluate");
            if (center_step - 1..=center_step + 1).contains(&candidate_step) {
                assert_eq!(matched, Some(candidate_step));
            } else {
                assert_eq!(matched, None);
            }
        }
    }

    #[test]
    fn manager_totp_selects_largest_step_when_multiple_candidates_match() {
        let matched = select_max_matching_step(100, |candidate| candidate != 100)
            .expect("simulated duplicate candidates should evaluate");
        assert_eq!(matched, Some(101));
    }

    #[test]
    fn generated_manager_totp_secret_round_trips_and_uri_is_explicit() {
        let provisioning =
            generate_manager_totp_provisioning().expect("provisioning should generate");
        assert_eq!(provisioning.manual_secret().expose().len(), 32);
        assert!(
            provisioning
                .manual_secret()
                .expose()
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        );
        validate_manager_totp_secret(provisioning.manual_secret())
            .expect("generated Base32 secret should decode to twenty bytes");
        assert_eq!(
            provisioning.otpauth_uri().expose(),
            format!(
                "otpauth://totp/Cyder:manager?secret={}&issuer=Cyder&algorithm=SHA1&digits=6&period=30",
                provisioning.manual_secret().expose()
            )
        );
        assert_eq!(
            format!("{provisioning:?}"),
            "ManagerTotpProvisioning(<redacted>)"
        );
        assert!(!format!("{provisioning:?}").contains(provisioning.manual_secret().expose()));
    }

    #[test]
    fn manager_totp_rejects_invalid_secret_code_and_timestamp() {
        let invalid_secret = SensitiveSecret::new("NOT-BASE32".to_string());
        assert_eq!(
            validate_manager_totp_secret(&invalid_secret),
            Err(ManagerTotpPrimitiveError::InvalidSecret)
        );

        let secret = SensitiveSecret::new(RFC_SHA1_SECRET_BASE32.to_string());
        for invalid_code in ["12345", "1234567", "１２３４５６", "12345A"] {
            assert_eq!(
                match_manager_totp_step(&secret, invalid_code, 1_000),
                Err(ManagerTotpPrimitiveError::InvalidCodeFormat)
            );
        }
        assert_eq!(
            match_manager_totp_step(&secret, "123456", -1),
            Err(ManagerTotpPrimitiveError::InvalidTimestamp)
        );
    }

    #[test]
    fn manager_totp_recovery_codes_have_unique_ids_and_normalize_safely() {
        let codes = generate_manager_totp_recovery_codes();
        assert_eq!(codes.len(), MANAGER_TOTP_RECOVERY_CODE_COUNT);
        let ids = codes
            .iter()
            .map(|code| code.code_id().to_string())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), MANAGER_TOTP_RECOVERY_CODE_COUNT);
        for code in &codes {
            assert_eq!(code.code_id().len(), 4);
            assert_eq!(code.normalized_for_hash().len(), 20);
            assert_eq!(code.display().expose().len(), 24);
            let lowercase = code.display().expose().to_ascii_lowercase();
            let (code_id, normalized) = normalize_manager_totp_recovery_code(&lowercase)
                .expect("lowercase hyphenated recovery code should normalize");
            assert_eq!(code_id, code.code_id());
            assert_eq!(normalized, code.normalized_for_hash());
            assert!(!format!("{code:?}").contains(code.display().expose()));
        }
        assert!(normalize_manager_totp_recovery_code("O000-0000-0000-0000-0000").is_err());
        assert!(normalize_manager_totp_recovery_code("TOO-SHORT").is_err());
    }
}
