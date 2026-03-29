import { defineStore } from "pinia";
import { ref } from "vue";

export const useAuthStore = defineStore("auth", () => {
  const accessToken = ref<string | null>(null);

  function setAccessToken(token: string | null) {
    accessToken.value = token;
  }

  return { accessToken, setAccessToken };
});
