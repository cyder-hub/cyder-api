<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  BellRing,
  Inbox,
  Loader2,
  Plus,
  RefreshCcw,
  Send,
  Trash2,
  Webhook,
} from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { confirm } from "@/lib/confirmController";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { formatTimestamp } from "@/lib/utils";
import { Api } from "@/services/request";
import type {
  NotificationChannel,
  NotificationDelivery,
  NotificationDeliveryStatus,
} from "@/store/types";

type DeliveryStatusFilter = NotificationDeliveryStatus | "all";

type ChannelDraft = {
  channel_key: string;
  name: string;
  endpoint_url: string;
  signing_secret: string;
  headers_json: string;
  clear_headers: boolean;
  cooldown_seconds: string;
  clear_signing_secret: boolean;
  is_enabled: boolean;
};

const { t: $t } = useI18n();

const channels = ref<NotificationChannel[]>([]);
const deliveries = ref<NotificationDelivery[]>([]);
const isLoading = ref(true);
const isRefreshing = ref(false);
const deliveryLoading = ref(false);
const saveLoading = ref(false);
const testLoadingId = ref<number | null>(null);
const error = ref<string | null>(null);
const deliveryError = ref<string | null>(null);
const isChannelDialogOpen = ref(false);
const editingChannel = ref<NotificationChannel | null>(null);
const deliveryStatus = ref<DeliveryStatusFilter>("failed");
const deliveryChannel = ref("all");
const draft = ref<ChannelDraft>(emptyDraft());

const statusOptions = computed(() => [
  { value: "failed", label: $t("notificationPage.delivery.status.failed") },
  { value: "retry_scheduled", label: $t("notificationPage.delivery.status.retry_scheduled") },
  { value: "skipped", label: $t("notificationPage.delivery.status.skipped") },
  { value: "in_progress", label: $t("notificationPage.delivery.status.in_progress") },
  { value: "pending", label: $t("notificationPage.delivery.status.pending") },
  { value: "succeeded", label: $t("notificationPage.delivery.status.succeeded") },
  { value: "all", label: $t("notificationPage.delivery.status.all") },
]);

const summaryCards = computed(() => {
  const enabled = channels.value.filter((channel) => channel.is_enabled).length;
  const failed = deliveries.value.filter((delivery) => delivery.status === "failed").length;
  const retrying = deliveries.value.filter(
    (delivery) => delivery.status === "retry_scheduled",
  ).length;
  return [
    { key: "channels", label: $t("notificationPage.summary.channels"), value: channels.value.length },
    { key: "enabled", label: $t("notificationPage.summary.enabled"), value: enabled },
    { key: "failed", label: $t("notificationPage.summary.failed"), value: failed },
    { key: "retrying", label: $t("notificationPage.summary.retrying"), value: retrying },
  ];
});

async function loadPage() {
  isRefreshing.value = true;
  error.value = null;
  try {
    channels.value = await Api.getNotificationChannels();
    await loadDeliveries();
  } catch (err) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    isLoading.value = false;
    isRefreshing.value = false;
  }
}

async function loadDeliveries() {
  deliveryLoading.value = true;
  deliveryError.value = null;
  try {
    const response = await Api.getNotificationDeliveries({
      status: deliveryStatus.value === "all" ? undefined : deliveryStatus.value,
      channel_id: deliveryChannel.value === "all" ? undefined : Number(deliveryChannel.value),
      limit: 50,
    });
    deliveries.value = response.items;
  } catch (err) {
    deliveryError.value = normalizeError(err, $t("common.unknownError")).message;
    deliveries.value = [];
  } finally {
    deliveryLoading.value = false;
  }
}

function openCreateDialog() {
  editingChannel.value = null;
  draft.value = emptyDraft();
  isChannelDialogOpen.value = true;
}

function openEditDialog(channel: NotificationChannel) {
  editingChannel.value = channel;
  draft.value = {
    channel_key: channel.channel_key,
    name: channel.name,
    endpoint_url: channel.endpoint_url,
    signing_secret: "",
    headers_json: channel.headers_json || "",
    clear_headers: false,
    cooldown_seconds: String(channel.cooldown_seconds),
    clear_signing_secret: false,
    is_enabled: channel.is_enabled,
  };
  isChannelDialogOpen.value = true;
}

async function saveChannel() {
  const trimmedName = draft.value.name.trim();
  const trimmedUrl = draft.value.endpoint_url.trim();
  const normalizedHeaders = normalizeHeadersDraft();
  if (normalizedHeaders === false) {
    return;
  }
  const cooldownSeconds = normalizeCooldownDraft();
  if (cooldownSeconds === null) {
    return;
  }
  if (!trimmedName || !trimmedUrl) {
    toastController.warn($t("notificationPage.toast.required"));
    return;
  }
  saveLoading.value = true;
  try {
    if (editingChannel.value) {
      await Api.updateNotificationChannel(editingChannel.value.id, {
        name: trimmedName,
        endpoint_url: trimmedUrl,
        signing_secret: draft.value.signing_secret.trim() || undefined,
        clear_signing_secret: draft.value.clear_signing_secret,
        headers_json: normalizedHeaders ?? undefined,
        clear_headers: draft.value.clear_headers,
        cooldown_seconds: cooldownSeconds,
        is_enabled: draft.value.is_enabled,
      });
      toastController.success($t("notificationPage.toast.updated"));
    } else {
      const channelKey = draft.value.channel_key.trim();
      if (!channelKey) {
        toastController.warn($t("notificationPage.toast.channelKeyRequired"));
        return;
      }
      await Api.createNotificationChannel({
        channel_key: channelKey,
        name: trimmedName,
        endpoint_url: trimmedUrl,
        signing_secret: draft.value.signing_secret.trim() || undefined,
        headers_json: normalizedHeaders ?? undefined,
        cooldown_seconds: cooldownSeconds,
        is_enabled: draft.value.is_enabled,
      });
      toastController.success($t("notificationPage.toast.created"));
    }
    isChannelDialogOpen.value = false;
    await loadPage();
  } catch (err) {
    toastController.error(
      $t("notificationPage.toast.saveFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    saveLoading.value = false;
  }
}

async function deleteChannel(channel: NotificationChannel) {
  if (!(await confirm($t("notificationPage.confirmDelete", { name: channel.name })))) {
    return;
  }
  try {
    await Api.deleteNotificationChannel(channel.id);
    toastController.success($t("notificationPage.toast.deleted"));
    await loadPage();
  } catch (err) {
    toastController.error(
      $t("notificationPage.toast.deleteFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
}

async function testChannel(channel: NotificationChannel) {
  testLoadingId.value = channel.id;
  try {
    const result = await Api.testNotificationChannel(channel.id);
    if (result.success) {
      toastController.success($t("notificationPage.toast.testSucceeded"));
    } else {
      toastController.error(
        result.error || $t("notificationPage.toast.testFailed"),
      );
    }
    await loadPage();
  } catch (err) {
    toastController.error(
      $t("notificationPage.toast.testFailedWithError", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    testLoadingId.value = null;
  }
}

function emptyDraft(): ChannelDraft {
  return {
    channel_key: "",
    name: "",
    endpoint_url: "",
    signing_secret: "",
    headers_json: "",
    clear_headers: false,
    cooldown_seconds: "900",
    clear_signing_secret: false,
    is_enabled: true,
  };
}

function normalizeHeadersDraft(): string | null | false {
  const raw = draft.value.headers_json.trim();
  if (!raw) {
    return null;
  }
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      toastController.warn($t("notificationPage.toast.headersInvalid"));
      return false;
    }
    return JSON.stringify(parsed);
  } catch {
    toastController.warn($t("notificationPage.toast.headersInvalid"));
    return false;
  }
}

function normalizeCooldownDraft(): number | null {
  const raw = draft.value.cooldown_seconds.trim();
  const value = raw ? Number(raw) : 900;
  if (!Number.isInteger(value) || value < 0 || value > 86400) {
    toastController.warn($t("notificationPage.toast.cooldownInvalid"));
    return null;
  }
  return value;
}

function channelStatusClass(channel: NotificationChannel) {
  return channel.is_enabled
    ? "border-emerald-200 bg-emerald-50 text-emerald-700"
    : "border-gray-200 bg-gray-100 text-gray-600";
}

function deliveryBadgeClass(status: NotificationDeliveryStatus) {
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

onMounted(() => {
  void loadPage();
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("notificationPage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("notificationPage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:items-center">
          <Button variant="outline" class="w-full sm:w-auto" :disabled="isRefreshing" @click="loadPage">
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
            {{ $t("common.refresh") }}
          </Button>
          <Button class="w-full sm:w-auto" @click="openCreateDialog">
            <Plus class="mr-1.5 h-4 w-4" />
            {{ $t("notificationPage.actions.newChannel") }}
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

      <div v-if="isLoading" class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("notificationPage.loading") }}</span>
      </div>
      <div v-else-if="error" class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ error }}
      </div>
      <template v-else>
        <section class="rounded-lg border border-gray-200 bg-white">
          <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 sm:flex-row sm:items-center sm:justify-between">
            <div class="min-w-0">
              <h2 class="text-base font-semibold text-gray-900">
                {{ $t("notificationPage.channels.title") }}
              </h2>
              <p class="mt-1 text-sm text-gray-500">
                {{ $t("notificationPage.channels.description") }}
              </p>
            </div>
          </div>

          <div v-if="!channels.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
            <Inbox class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
            <p class="text-sm font-medium">{{ $t("notificationPage.channels.empty") }}</p>
          </div>
          <div v-else class="divide-y divide-gray-100">
            <div
              v-for="channel in channels"
              :key="channel.id"
              class="grid grid-cols-1 gap-3 px-4 py-4 lg:grid-cols-[minmax(0,1fr)_auto]"
            >
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <Webhook class="h-4 w-4 text-gray-400" />
                  <p class="font-medium text-gray-900">{{ channel.name }}</p>
                  <Badge :class="channelStatusClass(channel)" class="font-mono text-[11px]">
                    {{
                      channel.is_enabled
                        ? $t("notificationPage.channels.enabled")
                        : $t("notificationPage.channels.disabled")
                    }}
                  </Badge>
                  <Badge class="border-gray-200 bg-white font-mono text-[11px] text-gray-600">
                    {{ channel.channel_key }}
                  </Badge>
                </div>
                <p class="mt-2 break-all font-mono text-xs text-gray-500">
                  {{ channel.endpoint_url }}
                </p>
                <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-gray-500">
                  <span>{{ $t("notificationPage.channels.secret") }} {{ channel.signing_secret_redacted ? $t("notificationPage.channels.configured") : $t("notificationPage.channels.notConfigured") }}</span>
                  <span>{{ $t("notificationPage.channels.headers") }} {{ channel.headers_json ? $t("notificationPage.channels.configured") : $t("notificationPage.channels.notConfigured") }}</span>
                  <span>{{ $t("notificationPage.channels.cooldown") }} {{ channel.cooldown_seconds }}s</span>
                  <span>{{ $t("notificationPage.channels.lastTest") }} {{ formatDateTime(channel.last_test_at) }}</span>
                  <span v-if="channel.last_test_error" class="break-words text-red-600">
                    {{ channel.last_test_error }}
                  </span>
                </div>
              </div>
              <div class="flex flex-col gap-2 sm:flex-row lg:justify-end">
                <Button variant="outline" size="sm" :disabled="testLoadingId === channel.id" @click="testChannel(channel)">
                  <Send class="mr-1.5 h-3.5 w-3.5" />
                  {{ $t("notificationPage.actions.test") }}
                </Button>
                <Button variant="ghost" size="sm" @click="openEditDialog(channel)">
                  {{ $t("common.edit") }}
                </Button>
                <Button variant="ghost" size="sm" class="text-gray-400 hover:text-red-600" @click="deleteChannel(channel)">
                  <Trash2 class="mr-1.5 h-3.5 w-3.5" />
                  {{ $t("common.delete") }}
                </Button>
              </div>
            </div>
          </div>
        </section>

        <section class="rounded-lg border border-gray-200 bg-white">
          <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 lg:flex-row lg:items-end lg:justify-between">
            <div class="min-w-0">
              <h2 class="text-base font-semibold text-gray-900">
                {{ $t("notificationPage.delivery.title") }}
              </h2>
              <p class="mt-1 text-sm text-gray-500">
                {{ $t("notificationPage.delivery.description") }}
              </p>
            </div>
            <div class="grid grid-cols-1 gap-2 sm:grid-cols-[10rem_12rem_auto]">
              <Select v-model="deliveryStatus" @update:model-value="loadDeliveries">
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent :body-lock="false">
                  <SelectItem v-for="option in statusOptions" :key="option.value" :value="option.value">
                    {{ option.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
              <Select v-model="deliveryChannel" @update:model-value="loadDeliveries">
                <SelectTrigger class="w-full">
                  <SelectValue :placeholder="$t('notificationPage.delivery.allChannels')" />
                </SelectTrigger>
                <SelectContent :body-lock="false">
                  <SelectItem value="all">{{ $t("notificationPage.delivery.allChannels") }}</SelectItem>
                  <SelectItem v-for="channel in channels" :key="channel.id" :value="String(channel.id)">
                    {{ channel.name }}
                  </SelectItem>
                </SelectContent>
              </Select>
              <Button variant="outline" :disabled="deliveryLoading" @click="loadDeliveries">
                <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': deliveryLoading }" />
                {{ $t("common.refresh") }}
              </Button>
            </div>
          </div>

          <p v-if="deliveryError" class="border-b border-red-100 bg-red-50 px-4 py-3 text-sm text-red-600">
            {{ deliveryError }}
          </p>
          <div v-if="deliveryLoading" class="flex items-center justify-center py-16 text-gray-500">
            <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
            <span class="text-sm">{{ $t("notificationPage.delivery.loading") }}</span>
          </div>
          <div v-else-if="!deliveries.length" class="flex flex-col items-center justify-center py-20 text-gray-500">
            <BellRing class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
            <p class="text-sm font-medium">{{ $t("notificationPage.delivery.empty") }}</p>
          </div>
          <div v-else class="app-scroll-x">
            <table class="min-w-full divide-y divide-gray-100 text-sm">
              <thead class="bg-gray-50/80">
                <tr>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.alert") }}
                  </th>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.status") }}
                  </th>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.attempt") }}
                  </th>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.nextAttempt") }}
                  </th>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.deliveredAt") }}
                  </th>
                  <th class="px-4 py-3 text-left text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("notificationPage.delivery.table.error") }}
                  </th>
                </tr>
              </thead>
              <tbody class="divide-y divide-gray-100 bg-white">
                <tr v-for="delivery in deliveries" :key="delivery.id">
                  <td class="px-4 py-3 align-top">
                    <p class="max-w-[18rem] truncate font-mono text-xs text-gray-700">
                      {{ delivery.alert_fingerprint }}
                    </p>
                    <p class="mt-1 font-mono text-xs text-gray-400">{{ delivery.event_type }}</p>
                  </td>
                  <td class="px-4 py-3 align-top">
                    <Badge :class="deliveryBadgeClass(delivery.status)" class="font-mono text-[11px]">
                      {{ $t(`notificationPage.delivery.status.${delivery.status}`) }}
                    </Badge>
                  </td>
                  <td class="px-4 py-3 align-top font-mono text-xs text-gray-700">
                    {{ delivery.attempt_count }}
                    <span v-if="delivery.last_status_code" class="text-gray-400">
                      / {{ delivery.last_status_code }}
                    </span>
                  </td>
                  <td class="px-4 py-3 align-top text-xs text-gray-600">
                    {{ formatDateTime(delivery.next_attempt_at) }}
                  </td>
                  <td class="px-4 py-3 align-top text-xs text-gray-600">
                    {{ formatDateTime(delivery.delivered_at) }}
                  </td>
                  <td class="max-w-[22rem] px-4 py-3 align-top">
                    <p class="break-words text-xs text-red-600">
                      {{ delivery.last_error || "-" }}
                    </p>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </template>

      <Dialog :open="isChannelDialogOpen" @update:open="(value) => (isChannelDialogOpen = value)">
        <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-xl">
          <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
            <DialogTitle class="text-lg font-semibold text-gray-900">
              {{
                editingChannel
                  ? $t("notificationPage.dialog.editTitle")
                  : $t("notificationPage.dialog.createTitle")
              }}
            </DialogTitle>
          </DialogHeader>
          <div class="space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.channelKey") }}
              </label>
              <Input v-model="draft.channel_key" :disabled="!!editingChannel" class="w-full" />
            </div>
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.name") }}
              </label>
              <Input v-model="draft.name" class="w-full" />
            </div>
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.endpointUrl") }}
              </label>
              <Input v-model="draft.endpoint_url" class="w-full" />
            </div>
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.signingSecret") }}
              </label>
              <Input v-model="draft.signing_secret" type="password" class="w-full" />
            </div>
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.headersJson") }}
              </label>
              <textarea
                v-model="draft.headers_json"
                class="min-h-24 w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-xs text-gray-800 outline-none transition focus:border-gray-400 focus:ring-2 focus:ring-gray-100"
                spellcheck="false"
              />
            </div>
            <div>
              <label class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("notificationPage.dialog.cooldownSeconds") }}
              </label>
              <Input v-model="draft.cooldown_seconds" type="number" min="0" max="86400" class="w-full" />
            </div>
            <label class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
              <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.enabled") }}</span>
              <Checkbox
                :model-value="draft.is_enabled"
                @update:model-value="(value) => (draft.is_enabled = value === true)"
              />
            </label>
            <label v-if="editingChannel" class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
              <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.clearSecret") }}</span>
              <Checkbox
                :model-value="draft.clear_signing_secret"
                @update:model-value="(value) => (draft.clear_signing_secret = value === true)"
              />
            </label>
            <label v-if="editingChannel" class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
              <span class="text-sm text-gray-700">{{ $t("notificationPage.dialog.clearHeaders") }}</span>
              <Checkbox
                :model-value="draft.clear_headers"
                @update:model-value="(value) => (draft.clear_headers = value === true)"
              />
            </label>
          </div>
          <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
            <Button variant="ghost" class="text-gray-600" @click="isChannelDialogOpen = false">
              {{ $t("common.cancel") }}
            </Button>
            <Button :disabled="saveLoading" @click="saveChannel">
              <Loader2 v-if="saveLoading" class="mr-1.5 h-4 w-4 animate-spin" />
              {{ $t("common.save") }}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  </div>
</template>
