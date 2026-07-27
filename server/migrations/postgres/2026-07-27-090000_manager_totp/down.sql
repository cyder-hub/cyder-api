DROP TABLE manager_totp_recovery_code;

ALTER TABLE manager_credential
    DROP CONSTRAINT chk_manager_credential_totp_all_or_none,
    DROP COLUMN totp_secret_ciphertext,
    DROP COLUMN totp_secret_nonce,
    DROP COLUMN totp_secret_format_version,
    DROP COLUMN totp_secret_key_fingerprint,
    DROP COLUMN totp_last_accepted_step,
    DROP COLUMN totp_enabled_at;
