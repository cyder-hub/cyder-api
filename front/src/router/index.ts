import { createRouter, createWebHistory } from "vue-router";
import DefaultLayout from "@/layouts/DefaultLayout.vue";
import LoginLayout from "@/layouts/LoginLayout.vue";
import { useAuthStore } from "@/store/authStore";
import { tryRefreshToken } from "@/services/auth";
import { readStoredRefreshToken } from "@/services/authTokens";

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
          redirect: { name: "Dashboard" },
        },
        {
          path: "dashboard",
          name: "Dashboard",
          component: () => import("@/pages/dashboard/DashboardPage.vue"),
          meta: {
            titleKey: "dashboard.title",
            navKey: "dashboard",
            navGroup: "operations",
            operatorPriority: "primary",
          },
        },
        {
          path: "api_key",
          name: "ApiKey",
          component: () => import("@/pages/api-key/ApiKeyPage.vue"),
          meta: {
            titleKey: "apiKeyPage.title",
            navKey: "apiKey",
            navGroup: "resources",
            operatorPriority: "secondary",
          },
        },
        {
          path: "model_route",
          name: "ModelRoute",
          component: () => import("@/pages/model-route/ModelRoutePage.vue"),
          meta: {
            titleKey: "modelRoutePage.title",
            navKey: "modelRoute",
            navGroup: "traffic",
            operatorPriority: "secondary",
          },
        },
        {
          path: "cost",
          name: "Cost",
          component: () => import("@/pages/cost/CostPage.vue"),
          meta: {
            titleKey: "costPage.title",
            navKey: "cost",
            navGroup: "governance",
            operatorPriority: "secondary",
          },
        },
        {
          path: "system/config",
          name: "SystemConfig",
          component: () => import("@/pages/system-config/SystemConfigPage.vue"),
          meta: {
            titleKey: "systemConfigPage.title",
            navKey: "systemConfig",
            navGroup: "governance",
            operatorPriority: "secondary",
          },
        },
        {
          path: "provider",
          name: "Provider",
          component: () => import("@/pages/provider/ProviderPage.vue"),
          meta: {
            titleKey: "providerPage.title",
            navKey: "provider",
            navGroup: "resources",
            operatorPriority: "secondary",
          },
        },
        {
          path: "model",
          name: "Model",
          component: () => import("@/pages/model/ModelPage.vue"),
          meta: {
            titleKey: "modelPage.title",
            navKey: "model",
            navGroup: "resources",
            operatorPriority: "secondary",
          },
        },
        {
          path: "provider/runtime",
          name: "ProviderRuntime",
          component: () => import("@/pages/provider-runtime/ProviderRuntimePage.vue"),
          meta: {
            titleKey: "providerRuntimePage.title",
            navKey: "providerRuntime",
            navGroup: "operations",
            operatorPriority: "secondary",
          },
        },
        {
          path: "alerts",
          name: "Alerts",
          component: () => import("@/pages/alerts/AlertsPage.vue"),
          meta: {
            titleKey: "alertsPage.title",
            navKey: "alerts",
            navGroup: "operations",
            operatorPriority: "secondary",
          },
        },
        {
          path: "notifications",
          name: "Notification",
          component: () => import("@/pages/notifications/NotificationsPage.vue"),
          meta: {
            titleKey: "notificationPage.title",
            navKey: "notifications",
            navGroup: "operations",
            operatorPriority: "secondary",
          },
        },
        {
          path: "record",
          name: "Record",
          component: () => import("@/pages/record/RecordPage.vue"),
          meta: {
            titleKey: "recordPage.title",
            navKey: "record",
            navGroup: "traffic",
            operatorPriority: "primary",
          },
        },
        {
          path: "provider/new",
          name: "ProviderNew",
          component: () => import("@/pages/provider-edit/ProviderEditPage.vue"),
          meta: {
            titleKey: "providerEditPage.titleAdd",
            navKey: "providerNew",
            parentNavKey: "provider",
            navGroup: "resources",
            operatorPriority: "detail",
          },
        },
        {
          path: "provider/edit/:id",
          name: "ProviderEdit",
          component: () => import("@/pages/provider-edit/ProviderEditPage.vue"),
          meta: {
            titleKey: "providerEditPage.titleEdit",
            navKey: "providerEdit",
            parentNavKey: "provider",
            navGroup: "resources",
            operatorPriority: "detail",
          },
        },
        {
          path: "model/edit/:id",
          name: "ModelEdit",
          component: () => import("@/pages/model-edit/ModelEditPage.vue"),
          meta: {
            titleKey: "modelEditPage.title",
            navKey: "modelEdit",
            parentNavKey: "model",
            navGroup: "resources",
            operatorPriority: "detail",
          },
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
          component: () => import("@/pages/login/LoginPage.vue"),
          meta: {
            titleKey: "loginPage.title",
          },
        },
      ],
    },
  ],
});

router.beforeEach(async (to, _from, next) => {
  const requiresAuth = to.matched.some((record) => record.meta.requiresAuth);
  const refreshToken = readStoredRefreshToken();
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
