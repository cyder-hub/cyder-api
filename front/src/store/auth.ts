import { defineStore } from "pinia";
import { ref, computed } from "vue";
import router from "@/router";
import { request } from "@/services/api";
import type { User } from "./types";

export const useAuthStore = defineStore("auth", () => {
  const user = ref<User | null>(null);
  const token = ref(localStorage.getItem("token"));

  const isAuthenticated = computed(() => !!token.value && !!user.value);

  function setUser(newUser: User | null) {
    user.value = newUser;
  }

  function setToken(newToken: string | null) {
    if (newToken) {
      localStorage.setItem("token", newToken);
      token.value = newToken;
    } else {
      localStorage.removeItem("token");
      token.value = null;
    }
  }

  async function login(username: string, password: string): Promise<void> {
    const response = await request.post("/auth/login", { username, password });
    const { token: newToken } = response.data;
    setToken(newToken);
    await fetchUser();
  }

  function logout() {
    setToken(null);
    setUser(null);
    if (router.currentRoute.value.path !== "/login") {
      router.push("/login");
    }
  }

  async function fetchUser() {
    if (token.value) {
      try {
        // In a real scenario, you would fetch user data from an API
        // const userData = await api.get('/me')
        const userData = { username: "Admin" };
        setUser(userData);
      } catch (e) {
        setToken(null);
        setUser(null);
      }
    }
  }

  return { user, isAuthenticated, setUser, logout, setToken, fetchUser, login };
});
