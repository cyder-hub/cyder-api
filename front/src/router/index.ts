import { createRouter, createWebHistory } from "vue-router";
import DefaultLayout from "@/layouts/DefaultLayout.vue";
import LoginLayout from "@/layouts/LoginLayout.vue";
import { useAuthStore } from "@/store/authStore";
import { tryRefreshToken } from "@/services/auth";

const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes: [
    {
      path: "/",
      component: DefaultLayout,
      meta: { requiresAuth: true },
      children: [
        {
          path: "",
          name: "Index",
          component: () => import("@/pages/Index.vue"),
          beforeEnter: (_to, _from, next) => {
            next({ name: "Dashboard" });
          },
        },
        {
          path: "dashboard",
          name: "Dashboard",
          component: () => import("@/pages/Dashboard.vue"),
        },
        {
          path: "access_control",
          name: "AccessControl",
          component: () => import("@/pages/AccessControl.vue"),
        },
        {
          path: "api_key",
          name: "ApiKey",
          component: () => import("@/pages/ApiKey.vue"),
        },
        {
          path: "custom_fields",
          name: "CustomFields",
          component: () => import("@/pages/CustomFields.vue"),
        },
        {
          path: "model_transform",
          name: "ModelTransform",
          component: () => import("@/pages/ModelTransform.vue"),
        },
        {
          path: "cost",
          name: "Cost",
          component: () => import("@/pages/Cost.vue"),
        },
        {
          path: "provider",
          name: "Provider",
          component: () => import("@/pages/Provider.vue"),
        },
        {
          path: "provider/runtime",
          name: "ProviderRuntime",
          component: () => import("@/pages/ProviderRuntime.vue"),
        },
        {
          path: "record",
          name: "Record",
          component: () => import("@/pages/Record.vue"),
        },
        {
          path: "provider/new",
          name: "ProviderNew",
          component: () => import("@/pages/ProviderEdit.vue"),
        },
        {
          path: "provider/edit/:id",
          name: "ProviderEdit",
          component: () => import("@/pages/ProviderEdit.vue"),
        },
        {
          path: "model/edit/:id",
          name: "ModelEdit",
          component: () => import("@/pages/ModelEdit.vue"),
        },
      ],
    },
    {
      path: "/:pathMatch(.*)*",
      name: "NotFound",
      component: () => import("@/pages/NotFound.vue"),
    },
    {
      path: "/login",
      component: LoginLayout,
      children: [
        {
          path: "",
          name: "Login",
          component: () => import("@/pages/Login.vue"),
        },
      ],
    },
  ],
});

router.beforeEach(async (to, _from, next) => {
  const requiresAuth = to.matched.some((record) => record.meta.requiresAuth);
  const refreshToken = localStorage.getItem("auth_token");
  const isAuthenticated = !!refreshToken;

  if (requiresAuth) {
    if (!isAuthenticated) {
      next({ name: "Login" });
    } else {
      const authStore = useAuthStore();
      // Proactively refresh access_token if missing (e.g., after reload)
      if (!authStore.accessToken) {
        const refreshed = await tryRefreshToken();
        if (refreshed) {
          next();
        } else {
          next({ name: "Login" });
        }
      } else {
        next();
      }
    }
  } else if (to.name === "Login" && isAuthenticated) {
    next({ name: "Dashboard" });
  } else {
    next();
  }
});

export default router;
