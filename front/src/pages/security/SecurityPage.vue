<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import {
  KeyRound,
  Loader2,
  LogOut,
  RefreshCcw,
  ShieldCheck,
  ShieldOff,
} from "lucide-vue-next";
import { useAppI18n } from "@/i18n";
import PageHeader from "@/components/PageHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import RotatePasswordDialog from "@/components/manager/RotatePasswordDialog.vue";
import { getTotpStatus } from "@/services/auth";
import type { ManagerTotpState, ManagerTotpStatus } from "@/services/types";
import { useAuthStore } from "@/store/authStore";
import { formatTimestamp } from "@/utils/datetime";
import LogoutAllDialog from "./components/LogoutAllDialog.vue";
import TotpDisableDialog from "./components/TotpDisableDialog.vue";
import TotpSetupDrawer, {
  type TotpSetupMode,
} from "./components/TotpSetupDrawer.vue";
import {
  effectiveManagerTotpState,
  reconcileManagerTotpStatus,
} from "./totpStatus";

const { t, locale } = useAppI18n();
const authStore = useAuthStore();
const status = ref<ManagerTotpStatus | null>(null);
const isLoading = ref(false);
const loadError = ref(false);
const setupOpen = ref(false);
const setupMode = ref<TotpSetupMode>("enroll");
const disableOpen = ref(false);
const passwordOpen = ref(false);
const logoutAllOpen = ref(false);
const setupDrawer = ref<InstanceType<typeof TotpSetupDrawer> | null>(null);
let statusReloadPending = false;

const totpState = computed<ManagerTotpState>(
  () => effectiveManagerTotpState(authStore.totpState, status.value),
);

const stateBadgeClass = computed(() => {
  if (totpState.value === "enabled") {
    return "border-emerald-200 bg-emerald-50 text-emerald-700";
  }
  if (totpState.value === "disabled") {
    return "border-amber-200 bg-amber-50 text-amber-700";
  }
  return "border-red-200 bg-red-50 text-red-700";
});

const stateIcon = computed(() =>
  totpState.value === "enabled" ? ShieldCheck : ShieldOff,
);

const enabledAt = computed(() => {
  const seconds = status.value?.enabled_at;
  return seconds
    ? formatTimestamp(seconds * 1_000, locale.value)
    : "";
});

const loadStatus = async () => {
  if (isLoading.value) {
    statusReloadPending = true;
    return;
  }
  isLoading.value = true;
  loadError.value = false;
  try {
    status.value = reconcileManagerTotpStatus(
      authStore.totpState,
      await getTotpStatus(),
    );
  } catch {
    loadError.value = true;
  } finally {
    isLoading.value = false;
    if (statusReloadPending) {
      statusReloadPending = false;
      void loadStatus();
    }
  }
};

const openSetup = (mode: TotpSetupMode) => {
  setupMode.value = mode;
  setupOpen.value = true;
};

const handleSetupCompleted = () => {
  status.value = {
    state: "enabled",
    enabled_at: Math.floor(Date.now() / 1_000),
  };
  authStore.setTotpState("enabled");
};

const handleDisableCompleted = () => {
  status.value = { state: "disabled" };
  authStore.setTotpState("disabled");
};

watch(
  () => authStore.totpState,
  (state) => {
    status.value = reconcileManagerTotpStatus(state, status.value);
  },
);
watch(
  () => authStore.accessToken,
  (accessToken, previousAccessToken) => {
    if (
      accessToken &&
      previousAccessToken &&
      accessToken !== previousAccessToken
    ) {
      void loadStatus();
    }
  },
);

onMounted(loadStatus);
onBeforeRouteLeave(() => {
  setupDrawer.value?.reset();
  setupOpen.value = false;
  disableOpen.value = false;
  passwordOpen.value = false;
  logoutAllOpen.value = false;
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell app-page-shell--narrow">
      <PageHeader :title="t('securityPage.title')">
        <template #meta>
          <p class="mt-1 max-w-2xl text-sm leading-6 text-gray-500">
            {{ t("securityPage.description") }}
          </p>
        </template>
        <template #actions>
          <Button variant="outline" class="w-full sm:w-auto" :disabled="isLoading" @click="loadStatus">
            <RefreshCcw class="h-4 w-4" :class="{ 'animate-spin': isLoading }" />
            {{ t("common.refresh") }}
          </Button>
        </template>
      </PageHeader>

      <div
        v-if="loadError"
        class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
      >
        {{ t("securityPage.loadFailed") }}
      </div>

      <div class="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <section class="px-4 py-5 sm:px-6">
          <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="flex min-w-0 gap-3">
              <div class="mt-0.5 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-md border border-gray-200 bg-gray-50 text-gray-600">
                <component :is="stateIcon" class="h-4 w-4" />
              </div>
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <h2 class="text-base font-semibold text-gray-900">
                    {{ t("securityPage.totp.title") }}
                  </h2>
                  <Badge variant="outline" :class="stateBadgeClass">
                    {{ t(`securityPage.states.${totpState}`) }}
                  </Badge>
                </div>
                <p class="mt-1 text-sm leading-6 text-gray-500">
                  {{ t(`securityPage.totp.${totpState}Description`) }}
                </p>
                <p v-if="totpState === 'enabled' && enabledAt" class="mt-1 text-xs text-gray-500">
                  {{ t("securityPage.totp.enabledAt", { date: enabledAt }) }}
                </p>
                <p v-if="totpState === 'unavailable'" class="mt-2 text-sm font-medium text-red-700">
                  {{ t("securityPage.totp.recoveryGuidance") }}
                </p>
              </div>
            </div>
            <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
              <Button
                v-if="totpState === 'disabled'"
                class="w-full sm:w-auto"
                @click="openSetup('enroll')"
              >
                {{ t("securityPage.totp.enable") }}
              </Button>
              <template v-else-if="totpState === 'enabled'">
                <Button variant="outline" class="w-full sm:w-auto" @click="openSetup('replace')">
                  {{ t("securityPage.totp.replace") }}
                </Button>
                <Button variant="ghost" class="w-full text-red-700 hover:text-red-800 sm:w-auto" @click="disableOpen = true">
                  {{ t("securityPage.totp.disable") }}
                </Button>
              </template>
              <Button v-else variant="outline" class="w-full sm:w-auto" disabled>
                {{ t("securityPage.unavailableAction") }}
              </Button>
            </div>
          </div>
        </section>

        <section class="border-t border-gray-100 px-4 py-5 sm:px-6">
          <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="flex min-w-0 gap-3">
              <div class="mt-0.5 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-md border border-gray-200 bg-gray-50 text-gray-600">
                <KeyRound class="h-4 w-4" />
              </div>
              <div class="min-w-0">
                <h2 class="text-base font-semibold text-gray-900">
                  {{ t("securityPage.password.title") }}
                </h2>
                <p class="mt-1 text-sm leading-6 text-gray-500">
                  {{ t("securityPage.password.description") }}
                </p>
              </div>
            </div>
            <Button
              variant="outline"
              class="w-full sm:w-auto"
              :disabled="totpState === 'unavailable'"
              @click="passwordOpen = true"
            >
              {{ t("securityPage.password.action") }}
            </Button>
          </div>
        </section>

        <section class="border-t border-gray-100 px-4 py-5 sm:px-6">
          <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="flex min-w-0 gap-3">
              <div class="mt-0.5 flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-md border border-gray-200 bg-gray-50 text-gray-600">
                <LogOut class="h-4 w-4" />
              </div>
              <div class="min-w-0">
                <h2 class="text-base font-semibold text-gray-900">
                  {{ t("securityPage.sessions.title") }}
                </h2>
                <p class="mt-1 text-sm leading-6 text-gray-500">
                  {{ t("securityPage.sessions.description") }}
                </p>
              </div>
            </div>
            <Button
              variant="outline"
              class="w-full sm:w-auto"
              :disabled="totpState === 'unavailable'"
              @click="logoutAllOpen = true"
            >
              {{ t("securityPage.sessions.action") }}
            </Button>
          </div>
        </section>
      </div>

      <div v-if="isLoading && !status" class="flex items-center justify-center py-4 text-sm text-gray-500">
        <Loader2 class="mr-2 h-4 w-4 animate-spin" />
        {{ t("common.loading") }}
      </div>
    </div>

    <TotpSetupDrawer
      ref="setupDrawer"
      v-model:open="setupOpen"
      :mode="setupMode"
      @completed="handleSetupCompleted"
    />
    <TotpDisableDialog
      v-model:open="disableOpen"
      @completed="handleDisableCompleted"
    />
    <RotatePasswordDialog
      v-model:open="passwordOpen"
      :totp-state="totpState"
    />
    <LogoutAllDialog
      v-model:open="logoutAllOpen"
      :totp-state="totpState"
    />
  </div>
</template>
