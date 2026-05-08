import { ref } from "vue";
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
      const success = await options.login(password.value);
      if (success) {
        await options.onSuccess();
      } else {
        error.value = options.translate("loginPage.loginFailed");
      }
    } catch {
      error.value = options.translate("loginPage.loginFailed");
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
