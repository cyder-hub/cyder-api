import type { Ref } from "vue";
import type { AppTranslate } from "@/i18n";

export interface UseLoginFormOptions {
  login: (password: string) => Promise<boolean>;
  translate: AppTranslate;
  onSuccess: () => unknown | Promise<unknown>;
}

export interface UseLoginFormReturn {
  password: Ref<string>;
  isLoading: Ref<boolean>;
  error: Ref<string | null>;
  handleLogin: () => Promise<void>;
}
