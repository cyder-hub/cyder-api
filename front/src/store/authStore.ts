import { defineStore } from "pinia";
import { ref } from "vue";
import type { ManagerBootstrapState, ManagerBootstrapStatus } from "@/services/types";

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
  const lifecycle = ref<AuthLifecycle>("unknown");
  const bootstrapState = ref<BootstrapLoadState>("unknown");
  let bootstrapRequest: Promise<BootstrapLoadState> | null = null;

  function setRestoring() {
    lifecycle.value = "restoring";
    accessToken.value = null;
  }

  function setUnknown() {
    lifecycle.value = "unknown";
    accessToken.value = null;
  }

  function setAuthenticated(token: string) {
    accessToken.value = token;
    lifecycle.value = "authenticated";
  }

  function setAnonymous() {
    accessToken.value = null;
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
    lifecycle,
    bootstrapState,
    setRestoring,
    setUnknown,
    setAuthenticated,
    setAnonymous,
    resolveBootstrapState,
    markBootstrapReady,
  };
});
