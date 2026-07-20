import { ref } from "vue";
import { authErrorCode } from "../../../services/authErrors.ts";
import type { UseLoginFormOptions, UseLoginFormReturn } from "../types";

export function useLoginForm(
  options: UseLoginFormOptions,
): UseLoginFormReturn {
  const password = ref("");
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  const handleLogin = async () => {
    if (isLoading.value) {
      return;
    }

    isLoading.value = true;
    error.value = null;

    try {
      await options.login(password.value);
      await options.onSuccess();
    } catch (caught) {
      const code = authErrorCode(caught);
      if (code === 1411) {
        await options.onUninitialized();
        return;
      }
      error.value = options.errorForCode(code);
    } finally {
      isLoading.value = false;
    }
  };

  return {
    password,
    isLoading,
    error,
    handleLogin,
  };
}
