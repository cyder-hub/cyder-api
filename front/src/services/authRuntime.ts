let restoreHandler: (() => Promise<boolean>) | null = null;
let recoverHandler: (() => Promise<string>) | null = null;
let revokeHandler: (() => void) | null = null;
let loginNavigationHandler: (() => void) | null = null;

export function registerAuthRecovery(
  restore: () => Promise<boolean>,
  recover: () => Promise<string>,
  revoke: () => void,
): void {
  restoreHandler = restore;
  recoverHandler = recover;
  revokeHandler = revoke;
}

export function restoreManagerSession(): Promise<boolean> {
  return restoreHandler?.() ?? Promise.resolve(false);
}

export function recoverManagerAccess(): Promise<string> {
  return (
    recoverHandler?.() ??
    Promise.reject(new Error("manager access recovery is unavailable"))
  );
}

export function revokeManagerSession(): void {
  revokeHandler?.();
}

export function registerLoginNavigation(handler: () => void): void {
  loginNavigationHandler = handler;
}

export function navigateToManagerLogin(): void {
  loginNavigationHandler?.();
}
