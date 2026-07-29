import type { Ref } from "vue";
import type {
  ManagerPasswordLoginResult,
  ManagerTotpRecoveryResult,
  ManagerTotpRecoverySetup,
} from "@/services/types";

export type LoginStage =
  | "password"
  | "totp"
  | "recovery_credentials"
  | "recovery_totp"
  | "recovery_codes";

export interface UseLoginFormOptions {
  login: (password: string) => Promise<ManagerPasswordLoginResult>;
  completeTotpLogin: (
    loginChallenge: string,
    totpCode: string,
  ) => Promise<void>;
  startRecovery: (
    password: string,
    recoveryCode: string,
  ) => Promise<ManagerTotpRecoverySetup>;
  confirmRecovery: (
    recoveryChallenge: string,
    totpCode: string,
  ) => Promise<ManagerTotpRecoveryResult>;
  errorForCode: (code: number | null) => string;
  onUninitialized: () => unknown | Promise<unknown>;
  onSuccess: () => unknown | Promise<unknown>;
}

export interface UseLoginFormReturn {
  stage: Ref<LoginStage>;
  password: Ref<string>;
  totpCode: Ref<string>;
  recoveryPassword: Ref<string>;
  recoveryCode: Ref<string>;
  recoveryManualSecret: Ref<string>;
  recoveryOtpauthUri: Ref<string>;
  recoveryCodes: Ref<string[]>;
  recoveryCodesSaved: Ref<boolean>;
  remainingSeconds: Ref<number>;
  isLoading: Ref<boolean>;
  error: Ref<string | null>;
  handleLogin: () => Promise<void>;
  beginPasswordStage: () => void;
  beginRecovery: () => void;
  dispose: () => void;
}
