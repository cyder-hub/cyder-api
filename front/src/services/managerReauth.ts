import { useAuthStore } from "@/store/authStore";
import type { ManagerReauthStatus } from "./types";
import { authErrorCode } from "./authErrors";

export const MANAGER_REAUTH_REQUIRED_CODE = 1491;
export const MANAGER_REAUTH_METHOD_CHANGED_CODE = 1492;

type ManagerReauthPresenter = () => Promise<boolean>;

let presenter: ManagerReauthPresenter | null = null;
let pendingPrompt: Promise<boolean> | null = null;

export class ManagerReauthCancelledError extends Error {
  constructor() {
    super("manager reauthentication was cancelled");
    this.name = "ManagerReauthCancelledError";
  }
}

export class ManagerReauthUnavailableError extends Error {
  constructor() {
    super("manager reauthentication is unavailable");
    this.name = "ManagerReauthUnavailableError";
  }
}

export function isManagerReauthCancelled(
  error: unknown,
): error is ManagerReauthCancelledError {
  return error instanceof ManagerReauthCancelledError;
}

export function hasValidSecretGovernanceReauth(
  reauth: ManagerReauthStatus | null,
  nowMs = Date.now(),
): boolean {
  return (
    reauth?.scope === "secret_governance" &&
    reauth.verified_until * 1000 > nowMs
  );
}

export function registerManagerReauthPresenter(
  nextPresenter: ManagerReauthPresenter,
): () => void {
  presenter = nextPresenter;
  return () => {
    if (presenter === nextPresenter) presenter = null;
  };
}

export async function ensureSecretGovernanceReauth(): Promise<void> {
  const store = useAuthStore();
  if (hasValidSecretGovernanceReauth(store.reauth)) return;
  if (!presenter) throw new ManagerReauthUnavailableError();

  pendingPrompt ??= presenter().finally(() => {
    pendingPrompt = null;
  });
  if (!(await pendingPrompt)) throw new ManagerReauthCancelledError();
  if (!hasValidSecretGovernanceReauth(store.reauth)) {
    throw new ManagerReauthUnavailableError();
  }
}

export async function runWithSecretGovernanceReauth<T>(
  action: () => Promise<T>,
): Promise<T> {
  const store = useAuthStore();
  await ensureSecretGovernanceReauth();
  try {
    return await action();
  } catch (error) {
    if (authErrorCode(error) !== MANAGER_REAUTH_REQUIRED_CODE) throw error;
    store.clearReauth();
  }

  await ensureSecretGovernanceReauth();
  return action();
}
