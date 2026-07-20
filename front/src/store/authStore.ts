import { defineStore } from "pinia";
import { ref } from "vue";
import type { ManagerBootstrapState, ManagerBootstrapStatus } from "@/services/types";

export type BootstrapLoadState =
  | "unknown"
  | "loading"
  | ManagerBootstrapState
  | "error";

export const useAuthStore = defineStore("auth", () => {
  const accessToken = ref<string | null>(null);
  const bootstrapState = ref<BootstrapLoadState>("unknown");
  let bootstrapRequest: Promise<BootstrapLoadState> | null = null;

  function setAccessToken(token: string | null) {
    accessToken.value = token;
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
    bootstrapState,
    setAccessToken,
    resolveBootstrapState,
    markBootstrapReady,
  };
});
