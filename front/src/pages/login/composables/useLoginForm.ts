import {
  getCurrentScope,
  onScopeDispose,
  ref,
} from "vue";
import { authErrorCode } from "../../../services/authErrors.ts";
import type {
  LoginStage,
  UseLoginFormOptions,
  UseLoginFormReturn,
} from "../types";

const CHALLENGE_TERMINAL_CODES = new Set([1477, 1478]);

export function useLoginForm(
  options: UseLoginFormOptions,
): UseLoginFormReturn {
  const stage = ref<LoginStage>("password");
  const password = ref("");
  const totpCode = ref("");
  const recoveryPassword = ref("");
  const recoveryCode = ref("");
  const recoveryManualSecret = ref("");
  const recoveryOtpauthUri = ref("");
  const recoveryCodes = ref<string[]>([]);
  const recoveryCodesSaved = ref(false);
  const remainingSeconds = ref(0);
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  let loginChallenge = "";
  let recoveryChallenge = "";
  let expiresAt = 0;
  let countdownTimer: ReturnType<typeof setInterval> | null = null;

  const stopCountdown = (): void => {
    if (countdownTimer !== null) {
      clearInterval(countdownTimer);
      countdownTimer = null;
    }
    expiresAt = 0;
    remainingSeconds.value = 0;
  };

  const clearRecoveryMaterial = (): void => {
    recoveryChallenge = "";
    recoveryManualSecret.value = "";
    recoveryOtpauthUri.value = "";
  };

  const clearRecoveryCodes = (): void => {
    recoveryCodes.value = [];
    recoveryCodesSaved.value = false;
  };

  const clearChallengeState = (): void => {
    stopCountdown();
    loginChallenge = "";
    clearRecoveryMaterial();
    totpCode.value = "";
  };

  const updateCountdown = (): void => {
    if (expiresAt === 0) return;
    remainingSeconds.value = Math.max(
      0,
      Math.ceil((expiresAt - Date.now()) / 1_000),
    );
    if (remainingSeconds.value > 0) return;

    const expiredStage = stage.value;
    clearChallengeState();
    stage.value =
      expiredStage === "recovery_totp" ? "recovery_credentials" : "password";
    error.value = options.errorForCode(1477);
  };

  const startCountdown = (expiresIn: number): void => {
    stopCountdown();
    const boundedSeconds = Math.max(1, Math.floor(expiresIn));
    expiresAt = Date.now() + boundedSeconds * 1_000;
    remainingSeconds.value = boundedSeconds;
    countdownTimer = setInterval(updateCountdown, 1_000);
  };

  const beginPasswordStage = (): void => {
    clearChallengeState();
    clearRecoveryCodes();
    password.value = "";
    recoveryPassword.value = "";
    recoveryCode.value = "";
    stage.value = "password";
    error.value = null;
  };

  const beginRecovery = (): void => {
    clearChallengeState();
    clearRecoveryCodes();
    password.value = "";
    recoveryPassword.value = "";
    recoveryCode.value = "";
    stage.value = "recovery_credentials";
    error.value = null;
  };

  const handleTerminalChallengeError = (
    code: number | null,
    fallbackStage: LoginStage,
  ): void => {
    if (!CHALLENGE_TERMINAL_CODES.has(code ?? -1)) return;
    clearChallengeState();
    stage.value = fallbackStage;
  };

  const submitPassword = async (): Promise<void> => {
    const result = await options.login(password.value);
    password.value = "";
    if (result.state === "authenticated") {
      await options.onSuccess();
      return;
    }
    loginChallenge = result.login_challenge;
    stage.value = "totp";
    startCountdown(result.expires_in);
  };

  const submitTotp = async (): Promise<void> => {
    if (!loginChallenge) {
      beginPasswordStage();
      error.value = options.errorForCode(1477);
      return;
    }
    try {
      await options.completeTotpLogin(loginChallenge, totpCode.value);
      clearChallengeState();
      await options.onSuccess();
    } catch (caught) {
      const code = authErrorCode(caught);
      handleTerminalChallengeError(code, "password");
      throw caught;
    } finally {
      totpCode.value = "";
    }
  };

  const submitRecoveryCredentials = async (): Promise<void> => {
    const setup = await options.startRecovery(
      recoveryPassword.value,
      recoveryCode.value,
    );
    recoveryPassword.value = "";
    recoveryCode.value = "";
    recoveryChallenge = setup.recovery_challenge;
    recoveryManualSecret.value = setup.manual_secret;
    recoveryOtpauthUri.value = setup.otpauth_uri;
    stage.value = "recovery_totp";
    startCountdown(setup.expires_in);
  };

  const submitRecoveryTotp = async (): Promise<void> => {
    if (!recoveryChallenge) {
      beginRecovery();
      error.value = options.errorForCode(1477);
      return;
    }
    try {
      const result = await options.confirmRecovery(
        recoveryChallenge,
        totpCode.value,
      );
      clearChallengeState();
      recoveryCodes.value = [...result.recovery_codes];
      recoveryCodesSaved.value = false;
      stage.value = "recovery_codes";
    } catch (caught) {
      const code = authErrorCode(caught);
      handleTerminalChallengeError(code, "recovery_credentials");
      throw caught;
    } finally {
      totpCode.value = "";
    }
  };

  const submitCurrentStage = async (): Promise<void> => {
    if (stage.value === "password") return submitPassword();
    if (stage.value === "totp") return submitTotp();
    if (stage.value === "recovery_credentials") {
      return submitRecoveryCredentials();
    }
    if (stage.value === "recovery_totp") return submitRecoveryTotp();
    if (!recoveryCodesSaved.value) return;
    clearRecoveryCodes();
    await options.onSuccess();
  };

  const handleLogin = async (): Promise<void> => {
    if (isLoading.value) return;

    isLoading.value = true;
    error.value = null;
    try {
      await submitCurrentStage();
    } catch (caught) {
      const code = authErrorCode(caught);
      if (stage.value === "password" && code === 1411) {
        await options.onUninitialized();
        return;
      }
      error.value = options.errorForCode(code);
    } finally {
      isLoading.value = false;
    }
  };

  const dispose = (): void => {
    clearChallengeState();
    clearRecoveryCodes();
    password.value = "";
    recoveryPassword.value = "";
    recoveryCode.value = "";
    error.value = null;
  };

  if (getCurrentScope()) {
    onScopeDispose(dispose);
  }

  return {
    stage,
    password,
    totpCode,
    recoveryPassword,
    recoveryCode,
    recoveryManualSecret,
    recoveryOtpauthUri,
    recoveryCodes,
    recoveryCodesSaved,
    remainingSeconds,
    isLoading,
    error,
    handleLogin,
    beginPasswordStage,
    beginRecovery,
    dispose,
  };
}
