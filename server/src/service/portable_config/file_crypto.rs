use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::{Rng, distr::Alphanumeric, rng};
use serde_json::Value;
use thiserror::Error;
use zeroize::Zeroizing;

use super::{
    digest::sha256_hex,
    schema::{FileProtectionMode, PortableFileProtectionStatus},
};

pub const PORTABLE_BACKUP_HEADER: &str = "CYDER-API-BACKUP";
pub const PORTABLE_BACKUP_END_PREFIX: &str = "CYDER-API-END sha256=";

const KDF_NAME: &str = "argon2id";
const CIPHER_NAME: &str = "xchacha20poly1305";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const GENERATED_PASSWORD_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableFileEncodeOptions {
    pub mode: FileProtectionMode,
    pub password: Option<String>,
    pub auto_generate_password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPortableFile {
    pub content: String,
    pub file_protection: FileProtectionMode,
    pub generated_password: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedPortableFile {
    pub plaintext: String,
    pub file_protection: PortableFileProtectionStatus,
}

#[derive(Debug, Error)]
pub enum PortableFileCryptoError {
    #[error("password is required to decrypt this portable backup")]
    PasswordRequired,
    #[error("portable backup password is empty")]
    EmptyPassword,
    #[error("invalid portable backup armor: {0}")]
    InvalidArmor(String),
    #[error("portable backup encrypted payload checksum mismatch")]
    IntegrityMismatch,
    #[error("failed to decrypt portable backup; password may be incorrect or file is corrupted")]
    DecryptFailed,
    #[error("failed to encrypt portable backup")]
    EncryptFailed,
    #[error("failed to derive portable backup encryption key: {0}")]
    KeyDerivation(String),
    #[error("invalid decrypted portable backup UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("invalid plaintext portable backup JSON: {0}")]
    InvalidPlaintextJson(#[from] serde_json::Error),
    #[error("invalid base64 portable backup field `{field}`: {message}")]
    Base64 { field: String, message: String },
}

pub fn detect_file_protection(content: &str) -> FileProtectionMode {
    if content.trim_start().starts_with(PORTABLE_BACKUP_HEADER) {
        FileProtectionMode::PasswordEncrypted
    } else {
        FileProtectionMode::Plaintext
    }
}

pub fn generate_export_password() -> String {
    rng()
        .sample_iter(&Alphanumeric)
        .take(GENERATED_PASSWORD_LEN)
        .map(char::from)
        .collect()
}

pub fn encode_portable_file(
    plaintext: &str,
    options: PortableFileEncodeOptions,
) -> Result<EncodedPortableFile, PortableFileCryptoError> {
    match options.mode {
        FileProtectionMode::Plaintext => {
            serde_json::from_str::<Value>(plaintext)?;
            Ok(EncodedPortableFile {
                content: plaintext.to_string(),
                file_protection: FileProtectionMode::Plaintext,
                generated_password: None,
            })
        }
        FileProtectionMode::PasswordEncrypted => {
            let generated_password = options
                .auto_generate_password
                .then(generate_export_password);
            let password = generated_password
                .as_deref()
                .or(options.password.as_deref())
                .ok_or(PortableFileCryptoError::PasswordRequired)?;
            let content = encrypt_portable_file(plaintext, password)?;
            Ok(EncodedPortableFile {
                content,
                file_protection: FileProtectionMode::PasswordEncrypted,
                generated_password,
            })
        }
    }
}

pub fn decode_portable_file(
    content: &str,
    password: Option<&str>,
) -> Result<DecodedPortableFile, PortableFileCryptoError> {
    match detect_file_protection(content) {
        FileProtectionMode::Plaintext => {
            serde_json::from_str::<Value>(content)?;
            Ok(DecodedPortableFile {
                plaintext: content.to_string(),
                file_protection: PortableFileProtectionStatus {
                    mode: FileProtectionMode::Plaintext,
                    requires_password: false,
                    decrypted: true,
                    integrity_checked: false,
                    integrity_valid: None,
                },
            })
        }
        FileProtectionMode::PasswordEncrypted => {
            let password = password.ok_or(PortableFileCryptoError::PasswordRequired)?;
            let plaintext = decrypt_portable_file(content, password)?;
            Ok(DecodedPortableFile {
                plaintext,
                file_protection: PortableFileProtectionStatus {
                    mode: FileProtectionMode::PasswordEncrypted,
                    requires_password: true,
                    decrypted: true,
                    integrity_checked: true,
                    integrity_valid: Some(true),
                },
            })
        }
    }
}

pub fn encrypt_portable_file(
    plaintext: &str,
    password: &str,
) -> Result<String, PortableFileCryptoError> {
    serde_json::from_str::<Value>(plaintext)?;
    let mut salt = [0_u8; SALT_LEN];
    let mut nonce = [0_u8; NONCE_LEN];
    rng().fill(&mut salt);
    rng().fill(&mut nonce);

    let key = derive_key(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| PortableFileCryptoError::EncryptFailed)?;
    let plaintext_bytes = Zeroizing::new(plaintext.as_bytes().to_vec());
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext_bytes,
                aad: PORTABLE_BACKUP_HEADER.as_bytes(),
            },
        )
        .map_err(|_| PortableFileCryptoError::EncryptFailed)?;

    let ciphertext_sha256 = sha256_hex(&ciphertext);
    Ok(format!(
        "{header}\nkdf={kdf}\ncipher={cipher}\nsalt={salt}\nnonce={nonce}\n\n{ciphertext}\n{end_prefix}{sha256}\n",
        header = PORTABLE_BACKUP_HEADER,
        kdf = KDF_NAME,
        cipher = CIPHER_NAME,
        salt = STANDARD.encode(salt),
        nonce = STANDARD.encode(nonce),
        ciphertext = STANDARD.encode(ciphertext),
        end_prefix = PORTABLE_BACKUP_END_PREFIX,
        sha256 = ciphertext_sha256,
    ))
}

pub fn decrypt_portable_file(
    content: &str,
    password: &str,
) -> Result<String, PortableFileCryptoError> {
    let armored = parse_armored_file(content)?;
    let key = derive_key(password, &armored.salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..])
        .map_err(|_| PortableFileCryptoError::DecryptFailed)?;
    let plaintext = cipher
        .decrypt(
            &XNonce::from(nonce_array(&armored.nonce)?),
            Payload {
                msg: &armored.ciphertext,
                aad: PORTABLE_BACKUP_HEADER.as_bytes(),
            },
        )
        .map_err(|_| PortableFileCryptoError::DecryptFailed)?;
    let plaintext = String::from_utf8(plaintext)?;
    serde_json::from_str::<Value>(&plaintext)?;
    Ok(plaintext)
}

struct ArmoredPortableFile {
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

fn parse_armored_file(content: &str) -> Result<ArmoredPortableFile, PortableFileCryptoError> {
    let normalized = content.replace("\r\n", "\n");
    let (metadata, body) = normalized.split_once("\n\n").ok_or_else(|| {
        PortableFileCryptoError::InvalidArmor("missing header/body separator".to_string())
    })?;
    let mut lines = metadata.lines();
    let header = lines
        .next()
        .ok_or_else(|| PortableFileCryptoError::InvalidArmor("missing file header".to_string()))?;
    if header.trim() != PORTABLE_BACKUP_HEADER {
        return Err(PortableFileCryptoError::InvalidArmor(
            "unexpected file header".to_string(),
        ));
    }

    let mut kdf = None;
    let mut cipher = None;
    let mut salt = None;
    let mut nonce = None;

    for line in lines {
        let (key, value) = line.split_once('=').ok_or_else(|| {
            PortableFileCryptoError::InvalidArmor(format!("invalid metadata line `{line}`"))
        })?;
        match key {
            "kdf" => kdf = Some(value.to_string()),
            "cipher" => cipher = Some(value.to_string()),
            "salt" => salt = Some(decode_base64_field("salt", value)?),
            "nonce" => nonce = Some(decode_base64_field("nonce", value)?),
            other => {
                return Err(PortableFileCryptoError::InvalidArmor(format!(
                    "unsupported metadata field `{other}`"
                )));
            }
        }
    }

    if kdf.as_deref() != Some(KDF_NAME) {
        return Err(PortableFileCryptoError::InvalidArmor(
            "unsupported kdf".to_string(),
        ));
    }
    if cipher.as_deref() != Some(CIPHER_NAME) {
        return Err(PortableFileCryptoError::InvalidArmor(
            "unsupported cipher".to_string(),
        ));
    }

    let salt =
        salt.ok_or_else(|| PortableFileCryptoError::InvalidArmor("missing salt".to_string()))?;
    if salt.len() != SALT_LEN {
        return Err(PortableFileCryptoError::InvalidArmor(
            "invalid salt length".to_string(),
        ));
    }
    let nonce =
        nonce.ok_or_else(|| PortableFileCryptoError::InvalidArmor("missing nonce".to_string()))?;
    if nonce.len() != NONCE_LEN {
        return Err(PortableFileCryptoError::InvalidArmor(
            "invalid nonce length".to_string(),
        ));
    }

    let body_lines = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if body_lines.len() < 2 {
        return Err(PortableFileCryptoError::InvalidArmor(
            "missing ciphertext or footer".to_string(),
        ));
    }

    let footer = body_lines
        .last()
        .expect("body_lines length checked above")
        .strip_prefix(PORTABLE_BACKUP_END_PREFIX)
        .ok_or_else(|| PortableFileCryptoError::InvalidArmor("missing footer".to_string()))?;
    let ciphertext_b64 = body_lines[..body_lines.len() - 1].join("");
    let ciphertext = decode_base64_field("ciphertext", &ciphertext_b64)?;
    if sha256_hex(&ciphertext) != footer {
        return Err(PortableFileCryptoError::IntegrityMismatch);
    }

    Ok(ArmoredPortableFile {
        salt,
        nonce,
        ciphertext,
    })
}

fn derive_key(
    password: &str,
    salt: &[u8],
) -> Result<Zeroizing<[u8; KEY_LEN]>, PortableFileCryptoError> {
    if password.is_empty() {
        return Err(PortableFileCryptoError::EmptyPassword);
    }

    let params = Params::new(19 * 1024, 2, 1, Some(KEY_LEN))
        .map_err(|err| PortableFileCryptoError::KeyDerivation(err.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let password_bytes = Zeroizing::new(password.as_bytes().to_vec());
    let mut key = Zeroizing::new([0_u8; KEY_LEN]);
    argon2
        .hash_password_into(&password_bytes, salt, &mut key[..])
        .map_err(|err| PortableFileCryptoError::KeyDerivation(err.to_string()))?;
    Ok(key)
}

fn nonce_array(nonce: &[u8]) -> Result<[u8; NONCE_LEN], PortableFileCryptoError> {
    nonce
        .try_into()
        .map_err(|_| PortableFileCryptoError::InvalidArmor("invalid nonce length".to_string()))
}

fn decode_base64_field(field: &str, value: &str) -> Result<Vec<u8>, PortableFileCryptoError> {
    STANDARD
        .decode(value)
        .map_err(|err| PortableFileCryptoError::Base64 {
            field: field.to_string(),
            message: err.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        PORTABLE_BACKUP_END_PREFIX, PortableFileCryptoError, PortableFileEncodeOptions,
        decode_portable_file, encode_portable_file,
    };
    use crate::service::portable_config::schema::FileProtectionMode;

    fn sample_bundle() -> &'static str {
        r#"{
            "schema_version": "cyder.portable.v1",
            "exported_at": 1778236800000,
            "cyder_version": "1.0.0",
            "modules": [
                {
                    "module_id": "provider_profile",
                    "module_version": 1,
                    "summary": {},
                    "items": {
                        "providers": [
                            {
                                "provider_key": "openai",
                                "api_key": "sk-provider-secret"
                            }
                        ]
                    }
                },
                {
                    "module_id": "api_keys",
                    "module_version": 1,
                    "summary": {},
                    "items": [
                        {
                            "name": "downstream",
                            "api_key": "ck-downstream-secret"
                        }
                    ]
                }
            ]
        }"#
    }

    #[test]
    fn plaintext_json_file_is_recognized() {
        let decoded = decode_portable_file(sample_bundle(), None).expect("plaintext decode");

        assert_eq!(decoded.file_protection.mode, FileProtectionMode::Plaintext);
        assert!(!decoded.file_protection.requires_password);
        assert!(decoded.file_protection.decrypted);
        assert_eq!(decoded.plaintext, sample_bundle());
    }

    #[test]
    fn password_encrypted_file_round_trips_and_hides_raw_keys() {
        let encoded = encode_portable_file(
            sample_bundle(),
            PortableFileEncodeOptions {
                mode: FileProtectionMode::PasswordEncrypted,
                password: Some("correct horse battery staple".to_string()),
                auto_generate_password: false,
            },
        )
        .expect("encrypted file");

        assert_eq!(
            encoded.file_protection,
            FileProtectionMode::PasswordEncrypted
        );
        assert!(!encoded.content.contains("sk-provider-secret"));
        assert!(!encoded.content.contains("ck-downstream-secret"));

        let decoded = decode_portable_file(&encoded.content, Some("correct horse battery staple"))
            .expect("decrypt");
        assert_eq!(decoded.plaintext, sample_bundle());
        assert_eq!(
            decoded.file_protection.mode,
            FileProtectionMode::PasswordEncrypted
        );
        assert_eq!(decoded.file_protection.integrity_valid, Some(true));
    }

    #[test]
    fn wrong_password_returns_clear_error() {
        let encoded = encode_portable_file(
            sample_bundle(),
            PortableFileEncodeOptions {
                mode: FileProtectionMode::PasswordEncrypted,
                password: Some("right-password".to_string()),
                auto_generate_password: false,
            },
        )
        .expect("encrypted file");

        let err = decode_portable_file(&encoded.content, Some("wrong-password"))
            .expect_err("wrong password should fail");

        assert!(matches!(err, PortableFileCryptoError::DecryptFailed));
    }

    #[test]
    fn encrypted_file_footer_integrity_mismatch_is_rejected() {
        let encoded = encode_portable_file(
            sample_bundle(),
            PortableFileEncodeOptions {
                mode: FileProtectionMode::PasswordEncrypted,
                password: Some("password".to_string()),
                auto_generate_password: false,
            },
        )
        .expect("encrypted file");
        let mut lines = encoded.content.lines().collect::<Vec<_>>();
        let footer = lines
            .iter_mut()
            .find(|line| line.starts_with(PORTABLE_BACKUP_END_PREFIX))
            .expect("footer line");
        *footer =
            "CYDER-API-END sha256=0000000000000000000000000000000000000000000000000000000000000000";
        let tampered = lines.join("\n");

        let err = decode_portable_file(&tampered, Some("password"))
            .expect_err("footer mismatch should fail");

        assert!(matches!(err, PortableFileCryptoError::IntegrityMismatch));
    }

    #[test]
    fn password_can_be_auto_generated_once_for_export() {
        let encoded = encode_portable_file(
            sample_bundle(),
            PortableFileEncodeOptions {
                mode: FileProtectionMode::PasswordEncrypted,
                password: None,
                auto_generate_password: true,
            },
        )
        .expect("encrypted file");
        let generated = encoded
            .generated_password
            .as_deref()
            .expect("generated password should be returned once");

        assert_eq!(generated.len(), 32);
        decode_portable_file(&encoded.content, Some(generated)).expect("generated password works");
    }
}
