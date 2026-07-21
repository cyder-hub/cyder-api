let restoreHandler: (() => Promise<boolean>) | null = null;
let loginNavigationHandler: (() => void) | null = null;

export function registerAuthRestoration(
  handler: () => Promise<boolean>,
): void {
  restoreHandler = handler;
}

export function restoreManagerSession(): Promise<boolean> {
  return restoreHandler?.() ?? Promise.resolve(false);
}

export function registerLoginNavigation(handler: () => void): void {
  loginNavigationHandler = handler;
}

export function navigateToManagerLogin(): void {
  loginNavigationHandler?.();
}
