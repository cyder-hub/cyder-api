import { ref } from "vue";

import type { ApiKeyReveal } from "../../../services/types";

/**
 * Component-scoped plaintext ownership for the API key page.
 *
 * These refs must never be moved into Pinia or browser storage. Issued secrets
 * are the one-time Create/Rotate response; revealed secrets belong only to the
 * currently open detail drawer.
 */
export function useApiKeySecretState() {
  const issuedSecret = ref<ApiKeyReveal | null>(null);
  const revealedSecret = ref<ApiKeyReveal | null>(null);

  function setIssuedSecret(secret: ApiKeyReveal | null) {
    issuedSecret.value = secret;
  }

  function setRevealedSecret(secret: ApiKeyReveal | null) {
    revealedSecret.value = secret;
  }

  function selectKey(id: number | null) {
    if (revealedSecret.value?.id !== id) {
      revealedSecret.value = null;
    }
  }

  function closeDrawer() {
    revealedSecret.value = null;
  }

  function leaveRoute() {
    issuedSecret.value = null;
    revealedSecret.value = null;
  }

  function logout() {
    issuedSecret.value = null;
    revealedSecret.value = null;
  }

  return {
    issuedSecret,
    revealedSecret,
    setIssuedSecret,
    setRevealedSecret,
    selectKey,
    closeDrawer,
    leaveRoute,
    logout,
  };
}
