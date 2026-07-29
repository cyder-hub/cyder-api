import type { AxiosRequestConfig } from "axios";

export const MANAGER_TOTP_CODE_HEADER = "X-Cyder-TOTP-Code";

export function sensitiveTotpRequestConfig(
  totpCode: string,
): AxiosRequestConfig {
  return {
    headers: {
      [MANAGER_TOTP_CODE_HEADER]: totpCode,
    },
    _skipAuthRetry: true,
  } as AxiosRequestConfig;
}
