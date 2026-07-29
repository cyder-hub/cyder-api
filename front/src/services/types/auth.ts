// ========== Auth Types ==========
export interface User {
  username: string;
}

export type ManagerTotpState = "disabled" | "enabled" | "unavailable";

export type ManagerReauthEvidence =
  | "password"
  | "totp"
  | "password_totp"
  | "recovery_totp";

export interface ManagerReauthStatus {
  scope: "secret_governance";
  method: ManagerReauthEvidence;
  verified_until: number;
}

export interface ManagerAuthAccess {
  access_token: string;
  totp_state: ManagerTotpState;
  reauth: ManagerReauthStatus | null;
}

export type ManagerPasswordLoginResult =
  | {
      state: "authenticated";
      access_token: string;
      totp_state: "disabled";
      reauth: ManagerReauthStatus;
    }
  | {
      state: "totp_required";
      login_challenge: string;
      expires_in: number;
    };

export interface ManagerTotpSetup {
  setup_challenge: string;
  manual_secret: string;
  otpauth_uri: string;
  expires_in: number;
}

export interface ManagerTotpRecoverySetup {
  recovery_challenge: string;
  manual_secret: string;
  otpauth_uri: string;
  expires_in: number;
}

export interface ManagerTotpLifecycleResult extends ManagerAuthAccess {
  recovery_codes?: string[];
}

export interface ManagerTotpRecoveryResult extends ManagerAuthAccess {
  recovery_codes: string[];
}

export interface ManagerTotpStatus {
  state: ManagerTotpState;
  enabled_at?: number;
}

export interface LogoutAllResult {
  revoked_sessions: number;
}

export type ManagerReauthRequest =
  | { method: "password"; password: string }
  | { method: "totp"; totp_code: string };

export type ManagerBootstrapState = "uninitialized" | "ready";

export interface ManagerBootstrapStatus {
  state: ManagerBootstrapState;
}
