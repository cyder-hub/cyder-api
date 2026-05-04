<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  BellRing,
  CheckCircle2,
  Inbox,
  Loader2,
  RefreshCcw,
  Search,
  ShieldOff,
  XCircle,
} from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { formatTimestamp } from "@/lib/utils";
import { Api } from "@/services/request";
import type {
  AlertEvent,
  AlertListParams,
  AlertScopeType,
  AlertSeverity,
  AlertStatus,
  NotificationDelivery,
} from "@/store/types";

type AlertFilterValue<T extends string> = T | "all";

const { t: $t } = useI18n();

const alerts = ref<AlertEvent[]>([]);
const deliveries = ref<NotificationDelivery[]>([]);
const selectedAlert = ref<AlertEvent | null>(null);
const isLoading = ref(true);
const isRefreshing = ref(false);
const isDetailLoading = ref(false);
const actionLoading = ref<string | null>(null);
const error = ref<string | null>(null);
const deliveryError = ref<string | null>(null);
const ackNote = ref("");
const suppressUntil = ref("");
const suppressReason = ref("");
const filters = ref({
  status: "active" as AlertFilterValue<AlertStatus>,
  severity: "all" as AlertFilterValue<AlertSeverity>,
  scope_type: "all" as AlertFilterValue<AlertScopeType>,
  acknowledged: "all" as AlertFilterValue<"yes" | "no">,
  suppressed: "all" as AlertFilterValue<"yes" | "no">,
  query: "",
});

const statusOptions = computed(() => [
  { value: "active", label: $t("alertsPage.filter.statusActive") },
  { value: "resolved", label: $t("alertsPage.filter.statusResolved") },
  { value: "all", label: $t("alertsPage.filter.allStatuses") },
]);

const severityOptions = computed(() => [
  { value: "all", label: $t("alertsPage.filter.allSeverities") },
  { value: "critical", label: $t("alertsPage.severity.critical") },
  { value: "warning", label: $t("alertsPage.severity.warning") },
  { value: "info", label: $t("alertsPage.severity.info") },
]);

const scopeOptions = computed(() => [
  { value: "all", label: $t("alertsPage.filter.allScopes") },
  { value: "provider", label: $t("alertsPage.scope.provider") },
  { value: "model", label: $t("alertsPage.scope.model") },
  { value: "api_key", label: $t("alertsPage.scope.api_key") },
  { value: "provider_api_key", label: $t("alertsPage.scope.provider_api_key") },
  { value: "provider_model", label: $t("alertsPage.scope.provider_model") },
  { value: "system", label: $t("alertsPage.scope.system") },
  { value: "global", label: $t("alertsPage.scope.global") },
]);

const booleanOptions = computed(() => [
  { value: "all", label: $t("alertsPage.filter.all") },
  { value: "yes", label: $t("common.yes") },
  { value: "no", label: $t("common.no") },
]);

const visibleAlerts = computed(() => {
  const query = filters.value.query.trim().toLowerCase();
  if (!query) return alerts.value;
  return alerts.value.filter((alert) =>
    [
      alert.fingerprint,
      alert.rule_key,
      alert.title,
      alert.summary,
      alert.scope_id,
    ].some((value) => value.toLowerCase().includes(query)),
  );
});

const summaryCards = computed(() => {
  const active = alerts.value.filter((alert) => alert.status === "active").length;
  const critical = alerts.value.filter((alert) => alert.severity === "critical").length;
  const suppressed = alerts.value.filter((alert) => isSuppressed(alert)).length;
  const acknowledged = alerts.value.filter((alert) => !!alert.acknowledged_at).length;
  return [
    { key: "active", label: $t("alertsPage.summary.active"), value: active },
    { key: "critical", label: $t("alertsPage.summary.critical"), value: critical },
    { key: "suppressed", label: $t("alertsPage.summary.suppressed"), value: suppressed },
    { key: "acknowledged", label: $t("alertsPage.summary.acknowledged"), value: acknowledged },
  ];
});

const canAcknowledge = computed(
  () => !!selectedAlert.value && !selectedAlert.value.acknowledged_at,
);
const canSuppress = computed(
  () => !!selectedAlert.value && selectedAlert.value.status === "active",
);
const canUnsuppress = computed(
  () => !!selectedAlert.value && isSuppressed(selectedAlert.value),
);
const canResolve = computed(
  () => !!selectedAlert.value && selectedAlert.value.status === "active",
);

function toListParams(): AlertListParams {
  const params: AlertListParams = { limit: 50, offset: 0 };
  if (filters.value.status !== "all") params.status = filters.value.status;
  if (filters.value.severity !== "all") params.severity = filters.value.severity;
  if (filters.value.scope_type !== "all") params.scope_type = filters.value.scope_type;
  if (filters.value.acknowledged !== "all") {
    params.acknowledged = filters.value.acknowledged === "yes";
  }
  if (filters.value.suppressed !== "all") {
    params.suppressed = filters.value.suppressed === "yes";
  }
  return params;
}

async function loadAlerts() {
  isRefreshing.value = true;
  error.value = null;
  try {
    const response = await Api.getAlerts(toListParams());
    alerts.value = response.items;
    if (selectedAlert.value) {
      const current = response.items.find((item) => item.id === selectedAlert.value?.id);
      if (current) {
        selectedAlert.value = current;
      } else {
        selectedAlert.value = response.items[0] ?? null;
      }
    } else {
      selectedAlert.value = response.items[0] ?? null;
    }
    if (selectedAlert.value) {
      await loadDeliveries(selectedAlert.value.id);
    } else {
      deliveries.value = [];
    }
  } catch (err) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    isLoading.value = false;
    isRefreshing.value = false;
  }
}

async function selectAlert(alert: AlertEvent) {
  selectedAlert.value = alert;
  ackNote.value = alert.acknowledged_note ?? "";
  suppressReason.value = alert.suppressed_reason ?? "";
  suppressUntil.value = alert.suppressed_until
    ? toDateTimeLocal(alert.suppressed_until)
    : "";
  await loadDeliveries(alert.id);
}

async function refreshSelectedAlert() {
  if (!selectedAlert.value) return;
  isDetailLoading.value = true;
  try {
    selectedAlert.value = await Api.getAlert(selectedAlert.value.id);
    await loadDeliveries(selectedAlert.value.id);
  } finally {
    isDetailLoading.value = false;
  }
}

async function loadDeliveries(alertId: number) {
  deliveryError.value = null;
  try {
    const response = await Api.getNotificationDeliveries({
      alert_id: alertId,
      limit: 10,
    });
    deliveries.value = response.items;
  } catch (err) {
    deliveryError.value = normalizeError(err, $t("common.unknownError")).message;
    deliveries.value = [];
  }
}

async function acknowledgeSelected() {
  if (!selectedAlert.value) return;
  actionLoading.value = "ack";
  try {
    selectedAlert.value = await Api.acknowledgeAlert(selectedAlert.value.id, {
      note: ackNote.value.trim() || null,
    });
    toastController.success($t("alertsPage.toast.acknowledged"));
    await loadAlerts();
  } catch (err) {
    toastController.error(
      $t("alertsPage.toast.actionFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    actionLoading.value = null;
  }
}

async function suppressSelected() {
  if (!selectedAlert.value) return;
  const until = parseDateTimeLocal(suppressUntil.value);
  if (!until || until <= Date.now()) {
    toastController.warn($t("alertsPage.toast.invalidSuppressUntil"));
    return;
  }
  actionLoading.value = "suppress";
  try {
    selectedAlert.value = await Api.suppressAlert(selectedAlert.value.id, {
      suppressed_until: until,
      reason: suppressReason.value.trim() || null,
    });
    toastController.success($t("alertsPage.toast.suppressed"));
    await loadAlerts();
  } catch (err) {
    toastController.error(
      $t("alertsPage.toast.actionFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    actionLoading.value = null;
  }
}

async function unsuppressSelected() {
  if (!selectedAlert.value) return;
  actionLoading.value = "unsuppress";
  try {
    selectedAlert.value = await Api.unsuppressAlert(selectedAlert.value.id);
    suppressUntil.value = "";
    suppressReason.value = "";
    toastController.success($t("alertsPage.toast.unsuppressed"));
    await loadAlerts();
  } catch (err) {
    toastController.error(
      $t("alertsPage.toast.actionFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    actionLoading.value = null;
  }
}

async function resolveSelected() {
  if (!selectedAlert.value) return;
  actionLoading.value = "resolve";
  try {
    selectedAlert.value = await Api.resolveAlert(selectedAlert.value.id);
    toastController.success($t("alertsPage.toast.resolved"));
    await loadAlerts();
  } catch (err) {
    toastController.error(
      $t("alertsPage.toast.actionFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    actionLoading.value = null;
  }
}

function isSuppressed(alert: AlertEvent) {
  return !!alert.suppressed_until && alert.suppressed_until > Date.now();
}

function statusBadgeClass(status: AlertStatus) {
  return status === "active"
    ? "border-gray-900 bg-gray-900 text-white"
    : "border-gray-200 bg-gray-100 text-gray-600";
}

function severityBadgeClass(severity: AlertSeverity) {
  switch (severity) {
    case "critical":
      return "border-red-200 bg-red-50 text-red-700";
    case "warning":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "info":
      return "border-gray-200 bg-gray-100 text-gray-600";
  }
}

function deliveryBadgeClass(status: NotificationDelivery["status"]) {
  switch (status) {
    case "succeeded":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "failed":
      return "border-red-200 bg-red-50 text-red-700";
    case "retry_scheduled":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "in_progress":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "skipped":
      return "border-gray-200 bg-gray-50 text-gray-600";
    case "pending":
      return "border-gray-200 bg-gray-100 text-gray-600";
  }
}

function formatDateTime(value: number | null | undefined) {
  return formatTimestamp(value) || "-";
}

function toDateTimeLocal(timestampMs: number) {
  const date = new Date(timestampMs);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(timestampMs - offset).toISOString().slice(0, 16);
}

function parseDateTimeLocal(value: string) {
  if (!value) return null;
  const timestamp = new Date(value).getTime();
  return Number.isNaN(timestamp) ? null : timestamp;
}

onMounted(() => {
  void loadAlerts();
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("alertsPage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("alertsPage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
          <Button variant="outline" class="w-full sm:w-auto" :disabled="isRefreshing" @click="loadAlerts">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
            {{ $t("common.refresh") }}
          </Button>
        </div>
      </div>

      <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-gray-200 bg-gray-100 sm:grid-cols-4">
        <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
          <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
            {{ card.label }}
          </p>
          <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
            {{ card.value }}
          </p>
        </div>
      </div>

      <div class="rounded-lg border border-gray-200 bg-white p-4">
        <div class="grid grid-cols-1 gap-3 lg:grid-cols-12">
          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.status") }}
            </span>
            <Select v-model="filters.status" @update:model-value="loadAlerts">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem v-for="option in statusOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.severity") }}
            </span>
            <Select v-model="filters.severity" @update:model-value="loadAlerts">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem v-for="option in severityOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.scope") }}
            </span>
            <Select v-model="filters.scope_type" @update:model-value="loadAlerts">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem v-for="option in scopeOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="lg:col-span-3">
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.search") }}
            </span>
            <div class="relative">
              <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
              <Input
                v-model="filters.query"
                class="w-full pl-9"
                :placeholder="$t('alertsPage.filter.searchPlaceholder')"
              />
            </div>
          </div>
        </div>
        <div class="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.acknowledged") }}
            </span>
            <Select v-model="filters.acknowledged" @update:model-value="loadAlerts">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem v-for="option in booleanOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div>
            <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("alertsPage.filter.suppressed") }}
            </span>
            <Select v-model="filters.suppressed" @update:model-value="loadAlerts">
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem v-for="option in booleanOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </div>

      <div v-if="isLoading" class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("alertsPage.loading") }}</span>
      </div>
      <div v-else-if="error" class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ error }}
      </div>
      <div v-else class="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_26rem]">
        <div class="overflow-hidden rounded-lg border border-gray-200 bg-white">
          <div v-if="!visibleAlerts.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
            <Inbox class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
            <p class="text-sm font-medium">{{ $t("alertsPage.empty") }}</p>
          </div>
          <div v-else class="divide-y divide-gray-100">
            <button
              v-for="alert in visibleAlerts"
              :key="alert.id"
              type="button"
              class="block w-full px-4 py-4 text-left transition-colors hover:bg-gray-50"
              :class="selectedAlert?.id === alert.id ? 'bg-gray-50' : 'bg-white'"
              @click="selectAlert(alert)"
            >
              <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge :class="severityBadgeClass(alert.severity)" class="font-mono text-[11px]">
                      {{ $t(`alertsPage.severity.${alert.severity}`) }}
                    </Badge>
                    <Badge :class="statusBadgeClass(alert.status)" class="font-mono text-[11px]">
                      {{ $t(`alertsPage.status.${alert.status}`) }}
                    </Badge>
                    <Badge v-if="isSuppressed(alert)" class="border-gray-200 bg-gray-100 font-mono text-[11px] text-gray-600">
                      {{ $t("alertsPage.flags.suppressed") }}
                    </Badge>
                    <Badge v-if="alert.acknowledged_at" class="border-gray-200 bg-white font-mono text-[11px] text-gray-600">
                      {{ $t("alertsPage.flags.acknowledged") }}
                    </Badge>
                  </div>
                  <p class="mt-2 truncate text-sm font-medium text-gray-900">
                    {{ alert.title }}
                  </p>
                  <p class="mt-1 line-clamp-2 text-sm text-gray-500">
                    {{ alert.summary }}
                  </p>
                </div>
                <div class="shrink-0 text-left text-xs text-gray-500 sm:text-right">
                  <p class="font-mono">{{ alert.rule_key }}</p>
                  <p class="mt-1">{{ formatDateTime(alert.last_seen_at) }}</p>
                </div>
              </div>
            </button>
          </div>
        </div>

        <aside class="rounded-lg border border-gray-200 bg-white">
          <div v-if="!selectedAlert" class="flex flex-col items-center justify-center px-6 py-20 text-gray-500">
            <BellRing class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
            <p class="text-sm font-medium">{{ $t("alertsPage.detail.empty") }}</p>
          </div>
          <div v-else class="divide-y divide-gray-100">
            <div class="px-4 py-4">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="text-base font-semibold text-gray-900">{{ selectedAlert.title }}</p>
                  <p class="mt-1 break-words text-sm text-gray-500">{{ selectedAlert.summary }}</p>
                </div>
                <Button variant="ghost" size="sm" :disabled="isDetailLoading" @click="refreshSelectedAlert">
                  <RefreshCcw class="h-4 w-4" :class="{ 'animate-spin': isDetailLoading }" />
                </Button>
              </div>
              <dl class="mt-4 grid grid-cols-1 gap-3 text-xs">
                <div>
                  <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.fingerprint") }}</dt>
                  <dd class="mt-1 break-all font-mono text-gray-700">{{ selectedAlert.fingerprint }}</dd>
                </div>
                <div class="grid grid-cols-2 gap-3">
                  <div>
                    <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.scope") }}</dt>
                    <dd class="mt-1 font-mono text-gray-700">{{ selectedAlert.scope_type }}:{{ selectedAlert.scope_id }}</dd>
                  </div>
                  <div>
                    <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.count") }}</dt>
                    <dd class="mt-1 font-mono text-gray-700">{{ selectedAlert.occurrence_count }}</dd>
                  </div>
                </div>
                <div class="grid grid-cols-2 gap-3">
                  <div>
                    <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.firstSeen") }}</dt>
                    <dd class="mt-1 text-gray-700">{{ formatDateTime(selectedAlert.first_seen_at) }}</dd>
                  </div>
                  <div>
                    <dt class="font-medium uppercase tracking-wide text-gray-400">{{ $t("alertsPage.detail.lastSeen") }}</dt>
                    <dd class="mt-1 text-gray-700">{{ formatDateTime(selectedAlert.last_seen_at) }}</dd>
                  </div>
                </div>
              </dl>
            </div>

            <div class="space-y-3 px-4 py-4">
              <div>
                <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("alertsPage.action.ackNote") }}
                </label>
                <textarea
                  v-model="ackNote"
                  rows="3"
                  class="min-h-20 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-400"
                  :placeholder="$t('alertsPage.action.ackNotePlaceholder')"
                />
                <Button class="mt-2 w-full" :disabled="!canAcknowledge || actionLoading === 'ack'" @click="acknowledgeSelected">
                  <CheckCircle2 class="mr-1.5 h-4 w-4" />
                  {{ $t("alertsPage.action.acknowledge") }}
                </Button>
              </div>

              <div class="space-y-2 rounded-lg border border-gray-200 p-3">
                <label class="block text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("alertsPage.action.suppressUntil") }}
                </label>
                <Input v-model="suppressUntil" type="datetime-local" class="w-full" />
                <Input
                  v-model="suppressReason"
                  class="w-full"
                  :placeholder="$t('alertsPage.action.suppressReasonPlaceholder')"
                />
                <div class="flex flex-col gap-2 sm:flex-row">
                  <Button variant="outline" class="w-full" :disabled="!canSuppress || actionLoading === 'suppress'" @click="suppressSelected">
                    <ShieldOff class="mr-1.5 h-4 w-4" />
                    {{ $t("alertsPage.action.suppress") }}
                  </Button>
                  <Button variant="ghost" class="w-full text-gray-600" :disabled="!canUnsuppress || actionLoading === 'unsuppress'" @click="unsuppressSelected">
                    {{ $t("alertsPage.action.unsuppress") }}
                  </Button>
                </div>
              </div>

              <Button variant="outline" class="w-full" :disabled="!canResolve || actionLoading === 'resolve'" @click="resolveSelected">
                <XCircle class="mr-1.5 h-4 w-4" />
                {{ $t("alertsPage.action.resolve") }}
              </Button>
            </div>

            <div class="px-4 py-4">
              <div class="mb-3 flex items-center justify-between gap-2">
                <h2 class="text-sm font-semibold text-gray-900">{{ $t("alertsPage.delivery.title") }}</h2>
                <Button variant="ghost" size="sm" @click="loadDeliveries(selectedAlert.id)">
                  <RefreshCcw class="h-4 w-4" />
                </Button>
              </div>
              <p v-if="deliveryError" class="mb-3 break-words text-xs text-red-600">{{ deliveryError }}</p>
              <div v-if="!deliveries.length" class="rounded-lg border border-gray-100 px-3 py-6 text-center text-sm text-gray-500">
                {{ $t("alertsPage.delivery.empty") }}
              </div>
              <div v-else class="space-y-2">
                <div v-for="delivery in deliveries" :key="delivery.id" class="rounded-lg border border-gray-100 px-3 py-2.5">
                  <div class="flex items-start justify-between gap-2">
                    <div class="min-w-0">
                      <p class="font-mono text-xs text-gray-700">{{ delivery.event_type }}</p>
                      <p class="mt-1 text-xs text-gray-500">
                        {{ $t("alertsPage.delivery.attempts", { count: delivery.attempt_count }) }}
                      </p>
                    </div>
                    <Badge :class="deliveryBadgeClass(delivery.status)" class="font-mono text-[11px]">
                      {{ $t(`alertsPage.delivery.status.${delivery.status}`) }}
                    </Badge>
                  </div>
                  <p class="mt-2 text-xs text-gray-500">
                    {{ $t("alertsPage.delivery.nextAttempt") }} {{ formatDateTime(delivery.next_attempt_at) }}
                  </p>
                  <p v-if="delivery.last_error" class="mt-1 break-words text-xs text-red-600">
                    {{ delivery.last_error }}
                  </p>
                </div>
              </div>
            </div>
          </div>
        </aside>
      </div>
    </div>
  </div>
</template>
