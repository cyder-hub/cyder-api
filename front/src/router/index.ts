import { createRouter, createWebHistory } from "vue-router";
import DefaultLayout from "@/layouts/DefaultLayout.vue";
import LoginLayout from "@/layouts/LoginLayout.vue";
import { useAuthStore } from "@/store/authStore";
import { getBootstrapStatus, restoreStoredSession } from "@/services/auth";
import { readStoredAuthSession } from "@/services/authTokens";
import { registerLoginNavigation } from "@/services/authRuntime";
import { decideAuthRoute, type AuthRouteKind } from "./auth-state";

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
    {
      path: "/bootstrap",
      component: LoginLayout,
      children: [
        {
          path: "",
          name: "Bootstrap",
          component: () => import("@/pages/bootstrap/BootstrapPage.vue"),
          meta: {
            titleKey: "bootstrapPage.title",
          },
        },
      ],
    },
    {
      path: "/:pathMatch(.*)*",
      name: "NotFound",
      component: () => import("@/pages/NotFound.vue"),
    },
  ],
});

registerLoginNavigation(() => {
  if (router.currentRoute.value.name !== "Login") {
    void router.push({ name: "Login" });
  }
});

router.beforeEach(async (to, _from, next) => {
  const authStore = useAuthStore();
  const bootstrapState = await authStore.resolveBootstrapState(getBootstrapStatus);
  const requiresAuth = to.matched.some((record) => record.meta.requiresAuth);
  const storedSession = readStoredAuthSession();
  const routeKind: AuthRouteKind =
    to.name === "Bootstrap"
      ? "bootstrap"
      : to.name === "Login"
        ? "login"
        : requiresAuth
          ? "protected"
          : "public";
  const decision = decideAuthRoute({
    bootstrapState,
    routeKind,
    hasStoredSession: !!storedSession,
    lifecycle: authStore.lifecycle,
  });

  if (decision === "bootstrap") {
    next({ name: "Bootstrap" });
    return;
  }
  if (decision === "login") {
    next({ name: "Login" });
    return;
  }
  if (decision === "dashboard") {
    next({ name: "Dashboard" });
    return;
  }
  if (decision === "restore") {
    const refreshed = await restoreStoredSession();
    if (!refreshed) {
      if (routeKind === "login") {
        next();
      } else {
        next({ name: "Login" });
      }
      return;
    }
    if (routeKind === "protected") {
      next();
    } else {
      next({ name: "Dashboard" });
    }
    return;
  }
  next();
});

export default router;
