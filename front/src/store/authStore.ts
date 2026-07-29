import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  ManagerBootstrapState,
  ManagerBootstrapStatus,
  ManagerReauthStatus,
  ManagerTotpState,
} from "@/services/types";

export type BootstrapLoadState =
  | "unknown"
  | "loading"
  | ManagerBootstrapState
  | "error";

export type AuthLifecycle =
  | "unknown"
  | "restoring"
  | "authenticated"
  | "anonymous";

export const useAuthStore = defineStore("auth", () => {
  const accessToken = ref<string | null>(null);
  const totpState = ref<ManagerTotpState | null>(null);
  const reauth = ref<ManagerReauthStatus | null>(null);
  const lifecycle = ref<AuthLifecycle>("unknown");
  const bootstrapState = ref<BootstrapLoadState>("unknown");
  let bootstrapRequest: Promise<BootstrapLoadState> | null = null;
  let reauthTimer: ReturnType<typeof setTimeout> | null = null;

  function clearReauth() {
    if (reauthTimer !== null) {
      clearTimeout(reauthTimer);
      reauthTimer = null;
    }
    reauth.value = null;
  }

  function setReauth(value: ManagerReauthStatus | null) {
    clearReauth();
    if (!value || value.verified_until * 1000 <= Date.now()) return;
    reauth.value = value;
    const expectedExpiry = value.verified_until;
    reauthTimer = setTimeout(() => {
      if (reauth.value?.verified_until === expectedExpiry) {
        reauth.value = null;
      }
      reauthTimer = null;
    }, Math.max(0, value.verified_until * 1000 - Date.now()));
  }

  function setRestoring() {
    lifecycle.value = "restoring";
    accessToken.value = null;
    totpState.value = null;
    clearReauth();
  }

  function setUnknown() {
    lifecycle.value = "unknown";
    accessToken.value = null;
    totpState.value = null;
    clearReauth();
  }

  function setAuthenticated(
    token: string,
    managerTotpState: ManagerTotpState,
    managerReauth: ManagerReauthStatus | null,
  ) {
    accessToken.value = token;
    totpState.value = managerTotpState;
    setReauth(managerTotpState === "unavailable" ? null : managerReauth);
    lifecycle.value = "authenticated";
  }

  function setTotpState(managerTotpState: ManagerTotpState) {
    totpState.value = managerTotpState;
    if (managerTotpState === "unavailable") clearReauth();
  }

  function setAnonymous() {
    accessToken.value = null;
    totpState.value = null;
    clearReauth();
    lifecycle.value = "anonymous";
  }

  async function resolveBootstrapState(
    loader: () => Promise<ManagerBootstrapStatus>,
    force = false,
  ): Promise<BootstrapLoadState> {
    if (!force && ["uninitialized", "ready", "error"].includes(bootstrapState.value)) {
      return bootstrapState.value;
    }
    if (bootstrapRequest) {
      return bootstrapRequest;
    }

    bootstrapState.value = "loading";
    bootstrapRequest = loader()
      .then((result) => {
        bootstrapState.value = result.state;
        return bootstrapState.value;
      })
      .catch(() => {
        bootstrapState.value = "error";
        return bootstrapState.value;
      })
      .finally(() => {
        bootstrapRequest = null;
      });
    return bootstrapRequest;
  }

  function markBootstrapReady() {
    bootstrapState.value = "ready";
  }

  return {
    accessToken,
    totpState,
    reauth,
    lifecycle,
    bootstrapState,
    setRestoring,
    setUnknown,
    setAuthenticated,
    setTotpState,
    setReauth,
    clearReauth,
    setAnonymous,
    resolveBootstrapState,
    markBootstrapReady,
  };
});
