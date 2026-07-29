import { ref } from "vue";

import type { ProviderApiKeyReveal } from "@/services/types";

/** Component-local plaintext ownership. Never move these refs into Pinia or storage. */
export function useProviderCredentialSecretState() {
  const draftSecret = ref("");
  const replacementKeyId = ref<number | null>(null);
  const revealedSecret = ref<ProviderApiKeyReveal | null>(null);

  function openCreate() {
    clearDialog();
  }

  function openReplace(keyId: number) {
    clearDialog();
    replacementKeyId.value = keyId;
  }

  function setRevealed(secret: ProviderApiKeyReveal | null) {
    revealedSecret.value = secret;
  }

  function clearDialog() {
    draftSecret.value = "";
    replacementKeyId.value = null;
  }

  function clearAll() {
    clearDialog();
    revealedSecret.value = null;
  }

  return {
    draftSecret,
    replacementKeyId,
    revealedSecret,
    openCreate,
    openReplace,
    setRevealed,
    clearDialog,
    providerChanged: clearAll,
    leaveRoute: clearAll,
    logout: clearAll,
  };
}
