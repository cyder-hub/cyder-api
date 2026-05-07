import type { Ref } from "vue";

export interface UseLoginFormOptions {
  login: (password: string) => Promise<boolean>;
  translate: (key: string) => string;
  onSuccess: () => unknown | Promise<unknown>;
}

export interface UseLoginFormReturn {
  password: Ref<string>;
  isLoading: Ref<boolean>;
  error: Ref<string | null>;
  handleLogin: () => Promise<void>;
}
